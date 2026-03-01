use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use std::net::ToSocketAddrs;

pub struct NslookupCommand;

impl NslookupCommand {
    fn n(key: &str) -> String { StegoStore::get(StringCategory::Network, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for NslookupCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("NSLOOKUP_NAME").into_boxed_str())
    }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "NSLOOKUP").into_boxed_str())
    }
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_NET").into_boxed_str())
    }
    fn usage(&self) -> &str {
        Box::leak(Self::c("NSLOOKUP_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".nslookup google.com"].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("NSLOOKUP_ALIAS1").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, mut args: Arguments) -> Result<()> {
        let domain = match args.next() {
            Some(d) => d,
            None => {
                http.create_message(msg.channel_id.into(), &format!("`{}`", Self::c("NSLOOKUP_USAGE"))).await?;
                return Ok(());
            }
        };

        let thinking_msg_json = http
            .create_message(msg.channel_id.into(), &format!("`{}`", Self::n("NSLOOKUP_LOOKING").replace("{}", domain))).await?;
        let thinking_msg: Message = serde_json::from_str(&thinking_msg_json).unwrap();

        // Perform DNS lookup using std::net
        let lookup_res = format!("{}:80", domain).to_socket_addrs();
        
        match lookup_res {
            Ok(addrs) => {
                let addr_list: Vec<String> = addrs.map(|a| format!("`{}`", a.ip())).collect();
                if addr_list.is_empty() {
                     http.update_message(thinking_msg.channel_id, thinking_msg.id, Some(format!("❌ `{}`", domain))).await?;
                } else {
                    let result_text = format!("**{} - `{}`**\n{}", Self::n("NSLOOKUP_RES"), domain, addr_list.join("\n"));
                    http.update_message(thinking_msg.channel_id, thinking_msg.id, Some(result_text)).await?;
                }
            },
            Err(_) => {
                http.update_message(thinking_msg.channel_id, thinking_msg.id, Some(Self::n("NSLOOKUP_ERR_FLAGS").replace("{}", domain))).await?;
            }
        }

        Ok(())
    }
}
