use anyhow::Result;
use std::sync::OnceLock;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

#[derive(Debug)]
pub struct AuthManager {
    config: crate::config::AuthConfig,
}

impl AuthManager {
    pub fn new(config: crate::config::AuthConfig) -> Self {
        Self { config }
    }

    pub async fn is_authorized(&self, http: &HttpClient, message: &Message) -> Result<bool> {
        if self.config.auth_all {
            return Ok(true);
        }
        if self.config.auth_roles {
            if self.has_allowed_role(http, message).await? {
                return Ok(true);
            }
        }
        if self.config.auth_user {
            if self.is_allowed_user(message).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn has_allowed_role(&self, _http: &HttpClient, _message: &Message) -> Result<bool> {
        // Cloudflare Proxy does not support guild_member lookup yet.
        // If you need role auth, implement get_member in HttpClient.
        Ok(false)
        /*
        let member_response = match http
            .guild_member(message.guild_id.unwrap(), message.author.id)
            .await
        {
            Ok(member) => member,
            Err(_) => return Ok(false),
        };
        let member =
            member_response
                .model()
                .await
                .unwrap_or_else(|_| twilight_model::guild::Member {
                    avatar: None,
                    communication_disabled_until: None,
                    deaf: false,
                    flags: twilight_model::guild::MemberFlags::empty(),
                    joined_at: None,
                    mute: false,
                    nick: None,
                    pending: false,
                    premium_since: None,
                    roles: Vec::new(),
                    user: message.author.clone(),
                });

        for role_id in &member.roles {
            let role_str = role_id.to_string();
            if self.config.allowed_roles.contains(&role_str) {
                return Ok(true);
            }
        }

        Ok(false)
        */
    }

    async fn is_allowed_user(&self, message: &Message) -> Result<bool> {
        let user_id = message.author.id.to_string();
        Ok(self.config.allowed_users.contains(&user_id))
    }

    pub async fn get_auth_status(&self, http: &HttpClient, message: &Message) -> String {
        if self.config.auth_all {
            return "**Authentication**: Disabled (everyone allowed)".to_string();
        }

        let mut status = String::new();

        if self.config.auth_roles {
            status.push_str("**Role auth**: Enabled\n");
            status.push_str(&format!(
                "   **Allowed roles**: {} roles\n",
                self.config.allowed_roles.len()
            ));
        }

        if self.config.auth_user {
            status.push_str("**User auth**: Enabled\n");
            status.push_str(&format!(
                "   **Allowed users**: {} users\n",
                self.config.allowed_users.len()
            ));
        }

        if let Ok(authorized) = self.is_authorized(http, message).await {
            if authorized {
                status.push_str("**Your status**: Authorized");
            } else {
                status.push_str("**Your status**: Not authorized");
            }
        }

        status
    }

    pub fn get_config(&self) -> &crate::config::AuthConfig {
        &self.config
    }
}

static AUTH_MANAGER: OnceLock<AuthManager> = OnceLock::new();

pub fn get_auth_manager() -> &'static AuthManager {
    AUTH_MANAGER.get().expect("Auth manager not initialized")
}

pub fn init_auth_manager(config: crate::config::AuthConfig) {
    AUTH_MANAGER
        .set(AuthManager::new(config))
        .expect("Auth manager already initialized");
}

pub async fn require_auth(http: &HttpClient, message: &Message) -> Result<()> {
    let auth_manager = get_auth_manager();

    if !auth_manager.is_authorized(http, message).await? {
        return Err(anyhow::anyhow!(
            "**Unauthorized**: You don't have permission to use this bot"
        ));
    }

    Ok(())
}
