use crate::commands::Arguments;
use crate::commands::BotCommand;
use crate::core::auth::*;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::embed::EmbedField;
use twilight_model::channel::message::Message;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct AuthCommand;

impl AuthCommand {
    fn s(key: &str) -> String { StegoStore::get(StringCategory::Core, key) }
}

#[async_trait]
impl BotCommand for AuthCommand {
    fn name(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "AUTH_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "AUTH").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "CAT_CORE").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "AUTH_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "AUTH_USAGE").into_boxed_str()) as &'static str
        ].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "AUTH_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "AUTH_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let auth_manager = get_auth_manager();
        let config = auth_manager.get_config();
        let mut fields = Vec::new();

        fields.push(EmbedField {
            name: Self::s("GEN_ACCESS"),
            value: if config.auth_all {
                Self::s("ENABLED_ALL")
            } else {
                Self::s("DISABLED_AUTH")
            },
            inline: false,
        });

        // Role auth
        let role_status_str = if config.auth_roles {
            Self::s("ENABLED")
        } else {
            Self::s("DISABLED")
        };

        fields.push(EmbedField {
            name: Self::s("ROLE_AUTH"),
            value: format!(
                "**{}**: {}\n**{}**: {}",
                Self::s("STATUS"),
                role_status_str,
                Self::s("ALLOWED_ROLES"),
                config.allowed_roles.len()
            ),
            inline: false,
        });

        // User auth
        let user_status_str = if config.auth_user {
            Self::s("ENABLED")
        } else {
            Self::s("DISABLED")
        };

        fields.push(EmbedField {
            name: Self::s("USER_AUTH"),
            value: format!(
                "**{}**: {}\n**{}**: {}",
                Self::s("STATUS"),
                user_status_str,
                Self::s("ALLOWED_USERS"),
                config.allowed_users.len()
            ),
            inline: false,
        });

        // User status
        let auth_status = auth_manager.get_auth_status(http, msg).await;
        fields.push(EmbedField {
            name: Self::s("YOUR_STATUS"),
            value: auth_status,
            inline: false,
        });

        let mut embed = EmbedBuilder::new()
            .title(Self::s("AUTH_STATUS"))
            .description(Self::s("AUTH_DESC"))
            .color(0x0099FF)
            .footer(EmbedFooterBuilder::new(Self::s("AUTH_SYSTEM")));

        for field in fields {
            embed = embed.field(field);
        }

        let embed = embed.build();

        // http_client::HttpClient??? create_message_embeds?€ ?? ??? create_message?? ??? ???
        let response = serde_json::json!({
            "embeds": [embed]
        }).to_string();
        
        http.create_message(msg.channel_id.get(), &response).await?;

        Ok(())
    }
}

