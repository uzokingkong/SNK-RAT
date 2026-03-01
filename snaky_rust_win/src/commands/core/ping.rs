// Cursed bracket
use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use std::time::Instant;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use std::boxed::Box;
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct PingCommand;

impl PingCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for PingCommand {
    fn name(&self) -> &str { Box::leak(Self::c("CMD_PING").into_boxed_str()) }
    fn description(&self) -> &str { Box::leak(StegoStore::get(StringCategory::Desc, "PING").into_boxed_str()) }
    fn category(&self) -> &str { Box::leak(Self::c("CAT_CORE").into_boxed_str()) }
    fn usage(&self) -> &str { Box::leak(Self::c("PING_USAGE").into_boxed_str()) }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![Box::leak(Self::c("PING_USAGE").into_boxed_str()) as &'static str].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![Box::leak(Self::c("PING_ALIAS1").into_boxed_str()) as &'static str].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let start_time = Instant::now();
        let response_text = http.create_message(msg.channel_id.get(), &Self::m("MSG_PINGING")).await?;
        
        let response: twilight_model::channel::message::Message = serde_json::from_str(&response_text)?;
        let latency = start_time.elapsed().as_millis();
        
        let pong = Self::m("MSG_PONG").replace("{}", &latency.to_string());
        http.update_message(msg.channel_id, response.id, Some(pong)).await?;

        Ok(())
    }
}
