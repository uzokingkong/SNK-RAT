use anyhow::{anyhow, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::Command;
use std::os::windows::process::CommandExt;

pub struct DeviceId;

impl DeviceId {
    pub fn get_device_channel_name() -> Result<String> {
        let username = Self::get_username()?;
        let hardware_id = Self::get_hardware_id()?;

        // Hash for hwid (8 chars)
        let mut hasher = DefaultHasher::new();
        hardware_id.hash(&mut hasher);
        let hash = hasher.finish();
        let short_id = format!("{:08x}", hash & 0xFFFFFFFF);

        // Username (rm spaces, special chars)
        let clean_username = username
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_lowercase();

        Ok(format!("{}-{}", clean_username, short_id))
    }

    pub fn get_device_id() -> String {
        match Self::get_hardware_id() {
             Ok(hwid) => {
                 let mut hasher = DefaultHasher::new();
                 hwid.hash(&mut hasher);
                 format!("{:08x}", hasher.finish() & 0xFFFFFFFF)
             },
             Err(_) => "unknown".to_string()
        }
    }

    // Get the curr username
    fn get_username() -> Result<String> {
        if cfg!(windows) {
            std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .map_err(|_| anyhow!("Could not get username"))
        } else {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .map_err(|_| anyhow!("Could not get username"))
        }
    }

    // Get hwid using diff methods per platform
    fn get_hardware_id() -> Result<String> {
        if cfg!(windows) {
            Self::get_windows_hardware_id()
        } else {
            Self::get_fallback_hardware_id()
        }
    }

    #[cfg(windows)]
    fn get_windows_hardware_id() -> Result<String> {
        if let Ok(cpu_id) = Self::run_wmi_query("SELECT ProcessorId FROM Win32_Processor") {
            // CPU
            if !cpu_id.trim().is_empty() {
                return Ok(cpu_id);
            }
        }

        if let Ok(mb_serial) = Self::run_wmi_query("SELECT SerialNumber FROM Win32_BaseBoard") {
            // Motherboard
            if !mb_serial.trim().is_empty() {
                return Ok(mb_serial);
            }
        }

        if let Ok(bios_serial) = Self::run_wmi_query("SELECT SerialNumber FROM Win32_BIOS") {
            // BIOS
            if !bios_serial.trim().is_empty() {
                return Ok(bios_serial);
            }
        }

        Self::get_fallback_hardware_id()
    }

    #[cfg(windows)]
    fn run_wmi_query(query: &str) -> Result<String> {
        let output = Command::new("wmic")
            .arg("/format:list")
            .arg(query)
            .creation_flags(0x08000000)
            .output()?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout);
            for line in result.lines() {
                if line.contains('=') {
                    let parts: Vec<&str> = line.split('=').collect();
                    if parts.len() == 2 && !parts[1].trim().is_empty() {
                        return Ok(parts[1].trim().to_string());
                    }
                }
            }
        }

        Err(anyhow!("WMI query failed or returned empty"))
    }

    fn get_fallback_hardware_id() -> Result<String> {
        let mut hasher = DefaultHasher::new();

        if let Ok(hostname) = std::env::var("COMPUTERNAME") {
            hostname.hash(&mut hasher);
        } else if let Ok(hostname) = std::env::var("HOSTNAME") {
            hostname.hash(&mut hasher);
        }

        std::env::consts::OS.hash(&mut hasher);
        std::env::consts::ARCH.hash(&mut hasher);

        if let Ok(user) = Self::get_username() {
            user.hash(&mut hasher);
        }

        let hash = hasher.finish();
        Ok(format!("fallback-{:016x}", hash))
    }
}

pub fn get_device_id() -> String {
    DeviceId::get_device_id()
}
