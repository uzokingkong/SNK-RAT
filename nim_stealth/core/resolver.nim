import winim/lean
import macros, strutils

type
    MY_LDR_DATA_TABLE_ENTRY {.pure.} = object
        InLoadOrderLinks*: LIST_ENTRY
        InMemoryOrderLinks*: LIST_ENTRY
        InInitializationOrderLinks*: LIST_ENTRY
        DllBase*: PVOID
        EntryPoint*: PVOID
        SizeOfImage*: ULONG
        Padding*: ULONG
        FullDllName*: UNICODE_STRING
        BaseDllName*: UNICODE_STRING

    MY_PEB_LDR_DATA {.pure.} = object
        Length*: ULONG
        Initialized*: ULONG
        SsHandle*: PVOID
        InLoadOrderModuleList*: LIST_ENTRY
        InMemoryOrderModuleList*: LIST_ENTRY
        InInitializationOrderModuleList*: LIST_ENTRY

    MY_PEB {.pure.} = object
        Reserved1*: array[2, BYTE]
        BeingDebugged*: BYTE
        Reserved2*: array[1, BYTE]
        Reserved3*: array[2, PVOID]
        Ldr*: ptr MY_PEB_LDR_DATA

{.emit: """
#include <windows.h>
#include <winternl.h>

void* get_peb_safe() {
#ifdef _WIN64
    return (void*)__readgsqword(0x60);
#else
    return (void*)__readfsdword(0x30);
#endif
}
""".}

proc get_peb_safe(): pointer {.importc: "get_peb_safe", nodecl.}

# Compile-time hashing macro
macro H*(s: static string): uint32 =
  var h = 5381.uint32
  for i in 0..<s.len:
    h = ((h shl 5) + h) + ord(s[i]).uint32
  return newLit(h)

proc hash_api*(s: string): uint32 =
  var h = 5381.uint32
  for i in 0..<s.len:
    h = ((h shl 5) + h) + ord(s[i]).uint32
  return h

proc get_peb*(): ptr MY_PEB {.inline.} =
  return cast[ptr MY_PEB](get_peb_safe())

proc get_module_base*(module_hash: uint32): pointer =
  let peb = get_peb()
  if peb == nil or peb.Ldr == nil: return nil
  
  let list = addr peb.Ldr.InLoadOrderModuleList
  var entry = list.Flink
  
  while entry != nil and entry != list:
    let module = cast[ptr MY_LDR_DATA_TABLE_ENTRY](entry)
    let base_name = module.BaseDllName.Buffer
    let base_len = module.BaseDllName.Length.int div 2
    
    if base_name != nil and base_len > 0:
        var h = 5381.uint32
        for i in 0..<base_len:
            var c = cast[ptr uint16](cast[uint](base_name) + (i * 2).uint)[]
            if c >= 'A'.uint16 and c <= 'Z'.uint16:
                c = c + ('a'.uint16 - 'A'.uint16)
            h = ((h shl 5) + h) + c.uint32
        
        if h == module_hash:
            return module.DllBase
            
    entry = entry.Flink
  return nil

proc get_proc_address_hashed*(module_base: pointer, api_hash: uint32): pointer =
    if module_base == nil: return nil
    let base = cast[uint](module_base)
    let dos_header = cast[ptr IMAGE_DOS_HEADER](base)
    let nt_header = cast[ptr IMAGE_NT_HEADERS64](base + dos_header.e_lfanew.uint)
    
    let export_dir_entry = nt_header.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT]
    let export_dir_rva = export_dir_entry.VirtualAddress
    let export_dir_size = export_dir_entry.Size
    if export_dir_rva == 0: return nil
    
    let export_dir = cast[ptr IMAGE_EXPORT_DIRECTORY](base + export_dir_rva.uint)
    let names = cast[ptr UncheckedArray[uint32]](base + export_dir.AddressOfNames.uint)
    let functions = cast[ptr UncheckedArray[uint32]](base + export_dir.AddressOfFunctions.uint)
    let ordinals = cast[ptr UncheckedArray[uint16]](base + export_dir.AddressOfNameOrdinals.uint)

    for i in 0..<export_dir.NumberOfNames.int:
        let name_ptr = cast[ptr UncheckedArray[byte]](base + names[i].uint)
        var h = 5381.uint32
        var j = 0
        while true:
            let b = name_ptr[j]
            if b == 0: break
            h = ((h shl 5) + h) + b.uint32
            j += 1
        
        if h == api_hash:
            let func_rva = functions[ordinals[i]]
            
            # Check for Forwarding
            let rva = cast[uint32](export_dir_rva)
            let size = cast[uint32](export_dir_size)
            if func_rva >= rva and func_rva < (rva + size):
                let forward_str = $cast[cstring](base + func_rva.uint)
                let parts = forward_str.split('.')
                if parts.len == 2:
                    let dll_name = parts[0] & ".dll"
                    let func_name = parts[1]
                    let h_module = get_module_base(hash_api(dll_name.toLowerAscii()))
                    return get_proc_address_hashed(h_module, hash_api(func_name))
                return nil
            
            return cast[pointer](base + func_rva.uint)
    return nil

proc get_proc_address_by_ordinal*(module: pointer, ordinal: uint32): pointer =
    if module == nil: return nil
    let base = cast[uint](module)
    let dos_header = cast[ptr IMAGE_DOS_HEADER](base)
    let nt_header = cast[ptr IMAGE_NT_HEADERS64](base + dos_header.e_lfanew.uint)
    
    let export_dir_entry = nt_header.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_EXPORT]
    let export_dir_rva = export_dir_entry.VirtualAddress
    let export_dir_size = export_dir_entry.Size
    if export_dir_rva == 0: return nil
    
    let export_dir = cast[ptr IMAGE_EXPORT_DIRECTORY](base + export_dir_rva.uint)
    let functions = cast[ptr UncheckedArray[uint32]](base + export_dir.AddressOfFunctions.uint)
    let base_ordinal = cast[uint32](export_dir.Base)
    
    if ordinal < base_ordinal or ordinal >= base_ordinal + cast[uint32](export_dir.NumberOfFunctions):
        return nil
        
    let func_rva = functions[ordinal - base_ordinal]
    if func_rva == 0: return nil

    let rva = cast[uint32](export_dir_rva)
    let size = cast[uint32](export_dir_size)
    if func_rva >= rva and func_rva < (rva + size):
        let forward_str = $cast[cstring](base + func_rva.uint)
        let parts = forward_str.split('.')
        if parts.len == 2:
            let dll_name = parts[0] & ".dll"
            let func_name = parts[1]
            let h_module = get_module_base(hash_api(dll_name.toLowerAscii()))
            return get_proc_address_hashed(h_module, hash_api(func_name))
        return nil
    
    return cast[pointer](base + func_rva.uint)