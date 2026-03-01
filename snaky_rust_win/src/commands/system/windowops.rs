use crate::commands::Arguments;
use crate::commands::BotCommand;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use crate::utils::window_ops::minimize_all_windows;

pub struct WindowOpsCommand;

#[async_trait]
impl BotCommand for WindowOpsCommand {
    fn name(&self) -> &str { "windowops" }
    fn description(&self) -> &str { "Minimize all visible windows" }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str { ".windowops" }
    fn examples(&self) -> &'static [&'static str] { &[".windowops"] }
    fn aliases(&self) -> &'static [&'static str] { &["minall", "hideall"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        _args: Arguments,
    ) -> Result<()> {
        minimize_all_windows();
        http.create_message(msg.channel_id.get(), "Minimizing all windows...").await?;
        Ok(())
    }
}
