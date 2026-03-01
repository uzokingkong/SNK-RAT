
use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct ClearCommand;

#[async_trait]
impl BotCommand for ClearCommand {
    fn name(&self) -> &str { "clear" }
    fn description(&self) -> &str { "Clear the chat channel messages" }
    fn category(&self) -> &str { "filesystem" }
    fn usage(&self) -> &str { ".clear [number]" }
    fn examples(&self) -> &'static [&'static str] { &[".clear", ".clear 10"] }
    fn aliases(&self) -> &'static [&'static str] { &["cls", "purge"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        http.create_message(msg.channel_id.into(), "INFO: Clear command is temporarily disabled during rewrite.").await?;
        Ok(())
    }
}



