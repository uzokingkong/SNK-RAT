use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use std::process;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct ExitCommand;

#[async_trait]
impl BotCommand for ExitCommand {
    fn name(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "EXIT_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "EXIT").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "CAT_CORE").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "EXIT_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "EXIT_USAGE").into_boxed_str()) as &'static str
        ].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "EXIT_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "EXIT_ALIAS2").into_boxed_str()) as &'static str,
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "EXIT_ALIAS3").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let msg_content = format!("**{}**", StegoStore::get(StringCategory::Core, "EXIT_MSG"));
        http.create_message(msg.channel_id.get(), &msg_content).await?;

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        process::exit(0);
    }
}

