use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

#[cfg(windows)]
use winapi::um::fileapi::{GetDiskFreeSpaceExW, GetDriveTypeW};
#[cfg(windows)]
use winapi::um::winbase::{
    DRIVE_UNKNOWN, DRIVE_NO_ROOT_DIR, DRIVE_REMOVABLE, DRIVE_FIXED, 
    DRIVE_REMOTE, DRIVE_CDROM, DRIVE_RAMDISK,
};

pub struct CheckDriveCommand;

#[async_trait]
impl BotCommand for CheckDriveCommand {
    fn name(&self) -> &str { "checkdrive" }
    fn description(&self) -> &str { "List all available drives on the system" }
    fn category(&self) -> &str { "filesystem" }
    fn usage(&self) -> &str { ".checkdrive" }
    fn examples(&self) -> &'static [&'static str] { &[".checkdrive"] }
    fn aliases(&self) -> &'static [&'static str] { &["drives", "listdrives"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, _args: Arguments) -> Result<()> {
        let drives = Self::get_drives_info()?;

        if drives.is_empty() {
            http.create_message(msg.channel_id.get(), "ERROR: No drives found.").await?;
            return Ok(());
        }

        let mut output = String::from("**Available Drives:**\n\n");

        for drive in drives {
            output.push_str(&format!(
                "**{}** - {}\n??**Total:** {}\n??**Used:** {}\n??**Free:** {}\n\n",
                drive.letter,
                drive.drive_type,
                Self::format_bytes(drive.total_space),
                Self::format_bytes(drive.used_space),
                Self::format_bytes(drive.free_space)
            ));
        }

        if output.len() > 1900 {
            let truncated = &output[..1900];
            http.create_message(msg.channel_id.get(), &format!("{}\n\n*(truncated)*", truncated)).await?;
        } else {
            http.create_message(msg.channel_id.get(), &output).await?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct DriveInfo {
    letter: String,
    drive_type: String,
    total_space: u64,
    free_space: u64,
    used_space: u64,
}

impl CheckDriveCommand {
    #[cfg(windows)]
    fn get_drives_info() -> Result<Vec<DriveInfo>> {
        let mut drives = Vec::new();

        for letter in b'A'..=b'Z' {
            let drive_letter = format!("{}:\\", letter as char);
            let drive_path: Vec<u16> = OsStr::new(&drive_letter)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            unsafe {
                let drive_type = GetDriveTypeW(drive_path.as_ptr());

                if drive_type == DRIVE_UNKNOWN || drive_type == DRIVE_NO_ROOT_DIR {
                    continue;
                }

                let type_str = match drive_type {
                    DRIVE_REMOVABLE => "Removable",
                    DRIVE_FIXED => "Fixed",
                    DRIVE_REMOTE => "Network",
                    DRIVE_CDROM => "CD-ROM",
                    DRIVE_RAMDISK => "RAM Disk",
                    _ => "Unknown",
                };

                let mut free_bytes: u64 = 0;
                let mut total_bytes: u64 = 0;
                let mut _available_bytes: u64 = 0;

                let result = GetDiskFreeSpaceExW(
                    drive_path.as_ptr(),
                    &mut _available_bytes as *mut u64 as *mut _,
                    &mut total_bytes as *mut u64 as *mut _,
                    &mut free_bytes as *mut u64 as *mut _,
                );

                if result != 0 {
                    let used_bytes = total_bytes.saturating_sub(free_bytes);
                    drives.push(DriveInfo {
                        letter: format!("{}:", letter as char),
                        drive_type: type_str.to_string(),
                        total_space: total_bytes,
                        free_space: free_bytes,
                        used_space: used_bytes,
                    });
                }
            }
        }

        Ok(drives)
    }

    #[cfg(not(windows))]
    fn get_drives_info() -> Result<Vec<DriveInfo>> {
        Err(anyhow::anyhow!(
            "Drive listing is only supported on Windows"
        ))
    }

    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}


