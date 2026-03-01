use crate::command_registry::get_registry;
use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::embed::EmbedField;
use twilight_model::channel::message::embed::EmbedFooter;
use twilight_model::channel::message::Message;
use twilight_util::builder::embed::EmbedBuilder;
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct HelpCommand;

impl HelpCommand {
    fn s(key: &str) -> String { StegoStore::get(StringCategory::Core, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for HelpCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("HELP_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "HELP").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_CORE").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("HELP_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".help", ".help ping", ".help upload"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("HELP_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("HELP_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        if args.is_empty() {
            self.show_all_commands(http, msg).await?;
        } else {
            let command_name = args.next().unwrap_or("");
            self.show_command_help(http, msg, command_name).await?;
        }

        Ok(())
    }
}

impl HelpCommand {
    async fn show_all_commands(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let registry = get_registry();
        let categories = registry.get_categories();

        let mut category_fields = Vec::new();

        for category in categories {
            let commands = registry.get_commands_by_category(&category);
            if !commands.is_empty() {
                let mut current_chunk = String::new();
                let mut part = 1;

                for cmd in commands {
                    let line = format!("**{}** - {}\n", cmd.name, cmd.description);
                    if current_chunk.len() + line.len() > 1000 {
                        // Push the current chunk and start a new one
                        let field_name = if part == 1 {
                            format!("**{} Commands**", category.to_uppercase())
                        } else {
                            format!("**{} Commands (pt. {})**", category.to_uppercase(), part)
                        };
                        
                        category_fields.push(EmbedField {
                            name: field_name,
                            value: current_chunk.trim_end().to_string(),
                            inline: false,
                        });
                        
                        current_chunk = line;
                        part += 1;
                    } else {
                        current_chunk.push_str(&line);
                    }
                }

                // Push whatever is left
                if !current_chunk.is_empty() {
                    let field_name = if part == 1 {
                        format!("**{} Commands**", category.to_uppercase())
                    } else {
                        format!("**{} Commands (pt. {})**", category.to_uppercase(), part)
                    };

                    category_fields.push(EmbedField {
                        name: field_name,
                        value: current_chunk.trim_end().to_string(),
                        inline: false,
                    });
                }
            }
        }

        let mut embed_builder = EmbedBuilder::new()
            .title(format!("**{}**", Self::s("HELP_TITLE")))
            .description(&Self::s("HELP_DESC"))
            .color(0x0099ff);

        for field in category_fields {
            embed_builder = embed_builder.field(field);
        }

        let embed = embed_builder
            .footer(EmbedFooter {
                text: Self::s("HELP_GENERAL"),
                icon_url: None,
                proxy_icon_url: None,
            })
            .build();
        
        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;

        Ok(())
    }

    async fn show_command_help(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        command_name: &str,
    ) -> Result<()> {
        let registry = get_registry();
        if let Some(metadata) = registry.get_metadata(command_name) {
            let aliases = if metadata.aliases.is_empty() {
                Self::s("HELP_NONE")
            } else {
                metadata.aliases.join(", ")
            };

            let examples = metadata.examples.join("\n");

            let embed = EmbedBuilder::new()
                .title(Self::s("HELP_FOR_FMT").replace("{}", &metadata.name))
                .description(metadata.description)
                .color(0x0099ff)
                .field(EmbedField {
                    name: Self::s("USAGE"),
                    value: metadata.usage,
                    inline: false,
                })
                .field(EmbedField {
                    name: Self::s("CATEGORY"),
                    value: metadata.category,
                    inline: false,
                })
                .field(EmbedField {
                    name: Self::s("ALIASES"),
                    value: aliases,
                    inline: false,
                })
                .field(EmbedField {
                    name: Self::s("EXAMPLES"),
                    value: examples,
                    inline: false,
                })
                .footer(EmbedFooter {
                    text: Self::s("HELP_GENERAL"),
                    icon_url: None,
                    proxy_icon_url: None,
                })
                .build();

            http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        } else {
            let err_msg = Self::s("ERR_NOT_FOUND_FMT").replace("{}", command_name);
            http.create_message(
                msg.channel_id.get(),
                &err_msg
            ).await?;
        }

        Ok(())
    }
}
