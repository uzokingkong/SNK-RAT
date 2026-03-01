
#[path = "../stealth.rs"]
mod stealth;

fn main() {
    println!("=== Stealth Engine Rust Debugger ===");
    
    // 1. Try Initialize
    println!("[1] Attempting stealth::init_stealth_engine()...");
    if stealth::init_stealth_engine() {
        println!("[+] SUCCESS: Stealth Engine is initialized and ready!");
        
        if let Some(engine) = stealth::get_engine() {
            println!("[+] Engine base address: 0x{:x}", engine.base_addr);
            println!("[+] Running engine.init()...");
            unsafe {
                let res = (engine.init)();
                println!("[+] engine.init() returned: {}", res);
            }
        }
    } else {
        println!("[-] FAILED: Stealth Engine initialization failed.");
    }
}
