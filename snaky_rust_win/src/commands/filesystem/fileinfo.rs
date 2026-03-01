use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::path::Path;
use std::time::SystemTime;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

pub struct FileInfoCommand;

#[async_trait]
impl BotCommand for FileInfoCommand {
    fn name(&self) -> &str { "fileinfo" }
    fn description(&self) -> &str { "Show detailed file/directory metadata and attributes" }
    fn category(&self) -> &str { "filesystem" }
    fn usage(&self) -> &str { ".fileinfo <path>" }
    fn examples(&self) -> &'static [&'static str] { 
        &[".fileinfo document.pdf", ".fileinfo C:\\Users\\Desktop\\folder"] 
    }
    fn aliases(&self) -> &'static [&'static str] { &["infofile", "stat"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let path_str_owned = args.rest();
        let path_str = path_str_owned.trim();

        if path_str.is_empty() {
            http.create_message(msg.channel_id.get(), "ERROR: Please provide a path. Usage: `.fileinfo <path>`").await?;
            return Ok(());
        }

        let path = Path::new(path_str);

        if !path.exists() {
            http.create_message(msg.channel_id.get(), &format!("ERROR: Path not found: `{}`", path_str)).await?;
            return Ok(());
        }

        let metadata = fs::metadata(path)?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown");

        let mut output = format!("**File Information:** `{}`\n\n", file_name);

        output.push_str(&format!("**Path:** `{}`\n", path.display()));

        let file_type = if metadata.is_dir() {
            "Directory"
        } else if metadata.is_file() {
            "File"
        } else {
            "Symlink/Other"
        };
        output.push_str(&format!("**Type:** {}\n", file_type));

        output.push_str(&format!("**Size:** {}\n", Self::format_bytes(metadata.len())));

        if let Ok(created) = metadata.created() {
            output.push_str(&format!("**Created:** {}\n", Self::format_time(created)));
        }

        if let Ok(modified) = metadata.modified() {
            output.push_str(&format!("**Modified:** {}\n", Self::format_time(modified)));
        }

        if let Ok(accessed) = metadata.accessed() {
            output.push_str(&format!("**Accessed:** {}\n", Self::format_time(accessed)));
        }

        #[cfg(windows)]
        {
            let attributes = Self::get_windows_attributes(&metadata);
            if !attributes.is_empty() {
                output.push_str(&format!("**Attributes:** {}\n", attributes));
            }
        }

        output.push_str(&format!("**Readonly:** {}\n", metadata.permissions().readonly()));

        if metadata.is_file() {
            if let Some(ext) = path.extension() {
                output.push_str(&format!("**Extension:** .{}\n", ext.to_string_lossy()));
            }
        }

        if metadata.is_dir() {
            match fs::read_dir(path) {
                Ok(entries) => {
                    let count = entries.count();
                    output.push_str(&format!("**Items:** {}\n", count));
                }
                Err(_) => {}
            }
        }

        http.create_message(msg.channel_id.get(), &output).await?;

        Ok(())
    }
}

impl FileInfoCommand {
    #[cfg(windows)]
    fn get_windows_attributes(metadata: &fs::Metadata) -> String {
        let file_attributes = metadata.file_attributes();
        let mut attrs = Vec::new();

        const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x20;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
        const FILE_ATTRIBUTE_READONLY: u32 = 0x1;
        const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
        const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;

        if file_attributes & FILE_ATTRIBUTE_HIDDEN != 0 {
            attrs.push("Hidden");
        }
        if file_attributes & FILE_ATTRIBUTE_SYSTEM != 0 {
            attrs.push("System");
        }
        if file_attributes & FILE_ATTRIBUTE_READONLY != 0 {
            attrs.push("ReadOnly");
        }
        if file_attributes & FILE_ATTRIBUTE_ARCHIVE != 0 {
            attrs.push("Archive");
        }
        if file_attributes & FILE_ATTRIBUTE_COMPRESSED != 0 {
            attrs.push("Compressed");
        }
        if file_attributes & FILE_ATTRIBUTE_ENCRYPTED != 0 {
            attrs.push("Encrypted");
        }

        if attrs.is_empty() {
            "Normal".to_string()
        } else {
            attrs.join(", ")
        }
    }

    fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        const TB: u64 = GB * 1024;

        if bytes >= TB {
            format!("{:.2} TB ({} bytes)", bytes as f64 / TB as f64, bytes)
        } else if bytes >= GB {
            format!("{:.2} GB ({} bytes)", bytes as f64 / GB as f64, bytes)
        } else if bytes >= MB {
            format!("{:.2} MB ({} bytes)", bytes as f64 / MB as f64, bytes)
        } else if bytes >= KB {
            format!("{:.2} KB ({} bytes)", bytes as f64 / KB as f64, bytes)
        } else {
            format!("{} bytes", bytes)
        }
    }

    fn format_time(time: SystemTime) -> String {
        use std::time::UNIX_EPOCH;
        
        match time.duration_since(UNIX_EPOCH) {
            Ok(duration) => {
                let secs = duration.as_secs();
                let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0);
                
                if let Some(dt) = datetime {
                    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                } else {
                    "Unknown".to_string()
                }
            }
            Err(_) => "Unknown".to_string(),
        }
    }
}


