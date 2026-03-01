use crate::commands::{Arguments, BotCommand};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use std::sync::Arc;
use std::{ffi::CStr, ptr::null_mut};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};
use twilight_model::channel::message::embed::EmbedField;
use tokio::time::{timeout, Duration};
use std::path::{Path, PathBuf};

use winapi::um::winspool::{
    EnumPrintersA, StartDocPrinterA, StartPagePrinter, EndPagePrinter, 
    EndDocPrinter, WritePrinter, OpenPrinterA, ClosePrinter,
    PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_2A, DOC_INFO_1A,
};
use winapi::shared::minwindef::DWORD;

const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50MB
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
const ALLOWED_EXTENSIONS: &[&str] = &["pdf", "txt", "doc", "docx", "jpg", "png", "xps"];

pub struct PrintCommand;

#[async_trait]
impl BotCommand for PrintCommand {
    fn name(&self) -> &str { "print" }
    fn description(&self) -> &str { "Print documents to specified printer" }
    fn category(&self) -> &str { "utility" }
    fn usage(&self) -> &str { ".print <file|url|attachment> <printer> OR .print devices" }
    fn examples(&self) -> &'static [&'static str] {
        &[
            ".print devices",
            ".print C:\\Users\\User\\Documents\\document.pdf \"Microsoft Print to PDF\"",
            ".print https://github.com/snake0071/snaky/doc.pdf \"HP LaserJet Pro\"",
            ".print attachment \"Printer Name\" (with file attached)",
        ]
    }
    fn aliases(&self) -> &'static [&'static str] { &["printer"] }

    async fn execute(&self, http: &Arc<HttpClient>, msg: &Message, args: Arguments) -> Result<()> {
        let rest_str = args.rest();
        let rest = rest_str.trim();
        
        if rest.is_empty() {
            self.show_help(http, msg).await?;
            return Ok(());
        }

        let (command, remaining) = rest.split_once(' ')
            .map(|(c, r)| (c, r.trim()))
            .unwrap_or((rest, ""));

        match command {
            "devices" | "list" => self.list_printers(http, msg).await,
            "attachment" | "attach" => {
                let printer = self.extract_printer_name(remaining)?;
                self.handle_attachment(http, msg, printer).await
            }
            _ => {
                let printer = self.extract_printer_name(remaining)?;
                
                if command.starts_with("http://") || command.starts_with("https://") {
                    self.print_from_url(http, msg, command, printer).await
                } else {
                    self.print_file_path(http, msg, command, printer).await
                }
            }
        }
    }
}

impl PrintCommand {
    async fn show_help(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let embed = EmbedBuilder::new()
            .title("Print Command")
            .description("Print documents to specified printers")
            .color(0x9370DB)
            .field(EmbedField {
                name: "Usage".to_string(),
                value: format!(
                    "`.print devices` - List available printers\n\
                     `.print <file> \"<printer>\"` - Print local file\n\
                     `.print <url> \"<printer>\"` - Print from URL\n\
                     `.print attachment \"<printer>\"` - Print attached file\n\n\
                     **Supported formats**: {}\n\
                     **Max file size**: {}MB",
                    ALLOWED_EXTENSIONS.join(", "),
                    MAX_FILE_SIZE / 1024 / 1024
                ),
                inline: false,
            })
            .footer(EmbedFooterBuilder::new("Snaky Print Command"))
            .build();

        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        Ok(())
    }

    fn extract_printer_name<'a>(&self, input: &'a str) -> Result<&'a str> {
        let input = input.trim();
        
        if input.is_empty() {
            bail!("Please specify a printer name. Use `.print devices` to see available printers.");
        }

        // support both "Printer Name" and Printer Name
        let printer = if input.starts_with('"') && input.ends_with('"') {
            &input[1..input.len()-1]
        } else {
            input
        };

        if printer.is_empty() {
            bail!("Printer name cannot be empty");
        }

        // validation
        if printer.contains(['\n', '\r', '\0']) {
            bail!("Invalid printer name");
        }

        Ok(printer)
    }

    fn validate_file_path(&self, path: &str) -> Result<PathBuf> {
        let path = Path::new(path);
        
        if !path.exists() {
            bail!("File not found: {}", path.display());
        }

        if !path.is_file() {
            bail!("Path is not a file: {}", path.display());
        }

        // check extension
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                bail!("File type '{}' is not supported. Allowed: {}", 
                    ext, ALLOWED_EXTENSIONS.join(", "));
            }
        } else {
            bail!("File has no extension");
        }

        // check file size
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > MAX_FILE_SIZE {
            bail!("File is too large. Max size: {}MB", MAX_FILE_SIZE / 1024 / 1024);
        }

        Ok(path.to_path_buf())
    }

    async fn list_printers(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let printers = Self::enumerate_printers()?;

        if printers.is_empty() {
            http.create_message(msg.channel_id.get(), "No printers found.").await?;
            return Ok(());
        }

        let description = printers
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}. `{}`", i + 1, p))
            .collect::<Vec<_>>()
            .join("\n");

        let embed = EmbedBuilder::new()
            .title("Available Printers")
            .description(description)
            .color(0x9370DB)
            .footer(EmbedFooterBuilder::new(&format!("Found {} printer(s)", printers.len())))
            .build();

        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        Ok(())
    }

    fn enumerate_printers() -> Result<Vec<String>> {
        unsafe {
            let mut needed: u32 = 0;
            let mut returned: u32 = 0;

            EnumPrintersA(
                PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
                null_mut(),
                2,
                null_mut(),
                0,
                &mut needed,
                &mut returned,
            );

            if needed == 0 {
                return Ok(Vec::new());
            }

            let mut buffer: Vec<u8> = vec![0u8; needed as usize];
            let result = EnumPrintersA(
                PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
                null_mut(),
                2,
                buffer.as_mut_ptr() as *mut _,
                needed,
                &mut needed,
                &mut returned,
            );

            if result == 0 {
                bail!("Failed to enumerate printers");
            }

            let infos: &[PRINTER_INFO_2A] = std::slice::from_raw_parts(
                buffer.as_ptr() as *const PRINTER_INFO_2A,
                returned as usize,
            );

            let mut printers = Vec::with_capacity(returned as usize);
            for info in infos {
                if !info.pPrinterName.is_null() {
                    let name = CStr::from_ptr(info.pPrinterName)
                        .to_string_lossy()
                        .into_owned();
                    printers.push(name);
                }
            }

            Ok(printers)
        }
    }

    async fn handle_attachment(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        printer: &str,
    ) -> Result<()> {
        if msg.attachments.is_empty() {
            http.create_message(msg.channel_id.get(), "**Error**: No file attached. Please attach a file.").await?;
            return Ok(());
        }

        let attachment = &msg.attachments[0];

        if attachment.size > MAX_FILE_SIZE {
            http.create_message(msg.channel_id.get(), &format!("**Error**: File is too large ({}MB). Max: {}MB", 
                    attachment.size / 1024 / 1024, MAX_FILE_SIZE / 1024 / 1024)).await?;
            return Ok(());
        }

        // validate extension
        if let Some(ext) = Path::new(&attachment.filename).extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                http.create_message(msg.channel_id.get(), &format!("**Error**: File type '{}' not supported. Allowed: {}", 
                        ext, ALLOWED_EXTENSIONS.join(", "))).await?;
                return Ok(());
            }
        }

        if msg.attachments.len() > 1 {
            http.create_message(msg.channel_id.get(), &format!("**Note**: Printing only first file: {}", attachment.filename)).await?;
        }

        self.download_and_print(http, msg, &attachment.url, &attachment.filename, printer).await
    }

    async fn print_file_path(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        file_path: &str,
        printer: &str,
    ) -> Result<()> {
        let path = match self.validate_file_path(file_path) {
            Ok(p) => p,
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("**Error**: {}", e)).await?;
                return Ok(());
            }
        };

        match self.print_file_raw(&path, printer) {
            Ok(_) => {
                let embed = EmbedBuilder::new()
                    .title("Print Job Sent")
                    .color(0x32CD32)
                    .field(EmbedField {
                        name: "File".to_string(),
                        value: format!("`{}`", path.display()),
                        inline: false,
                    })
                    .field(EmbedField {
                        name: "Printer".to_string(),
                        value: format!("`{}`", printer),
                        inline: false,
                    })
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("**Error**: Failed to print: {}", e)).await?;
            }
        }

        Ok(())
    }

    async fn print_from_url(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        url: &str,
        printer: &str,
    ) -> Result<()> {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            http.create_message(msg.channel_id.get(), "**Error**: Invalid URL").await?;
            return Ok(());
        }

        let file_name = url.split('/').last().unwrap_or("downloaded_file");
        
        // validate extension from URL
        if let Some(ext) = Path::new(file_name).extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
                http.create_message(msg.channel_id.get(), &format!("**Error**: File type '{}' not supported", ext)).await?;
                return Ok(());
            }
        }

        self.download_and_print(http, msg, url, file_name, printer).await
    }

    async fn download_and_print(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        url: &str,
        file_name: &str,
        printer: &str,
    ) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .build()?;

        let response = match timeout(DOWNLOAD_TIMEOUT, client.get(url).send()).await {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                http.create_message(msg.channel_id.get(), &format!("**Error**: Download failed: {}", e)).await?;
                return Ok(());
            }
            Err(_) => {
                http.create_message(msg.channel_id.get(), "**Error**: Download timeout").await?;
                return Ok(());
            }
        };

        if !response.status().is_success() {
            http.create_message(msg.channel_id.get(), &format!("**Error**: HTTP {}", response.status())).await?;
            return Ok(());
        }

        if let Some(len) = response.content_length() {
            if len > MAX_FILE_SIZE {
                http.create_message(msg.channel_id.get(), &format!("**Error**: File too large ({}MB)", len / 1024 / 1024)).await?;
                return Ok(());
            }
        }

        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("**Error**: Failed to read content: {}", e)).await?;
                return Ok(());
            }
        };

        let temp_path = std::env::temp_dir()
            .join(format!("snaky_print_{}_{}", msg.id.get(), file_name));

        let _cleanup = FileCleanup::new(temp_path.clone());

        std::fs::write(&temp_path, bytes)
            .context("Failed to save file")?;

        match self.print_file_raw(&temp_path, printer) {
            Ok(_) => {
                let embed = EmbedBuilder::new()
                    .title("Print Job Sent")
                    .color(0x32CD32)
                    .field(EmbedField {
                        name: "File".to_string(),
                        value: format!("`{}`", file_name),
                        inline: false,
                    })
                    .field(EmbedField {
                        name: "Printer".to_string(),
                        value: format!("`{}`", printer),
                        inline: false,
                    })
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &format!("**Error**: Failed to print: {}", e)).await?;
            }
        }

        Ok(())
    }

    fn print_file_raw(&self, file_path: &Path, printer_name: &str) -> Result<()> {
        let file_content = std::fs::read(file_path)
            .context("Failed to read file")?;

        unsafe {
            let mut h_printer: winapi::shared::ntdef::HANDLE = null_mut();
            let printer_cstr = std::ffi::CString::new(printer_name)?;

            let result = OpenPrinterA(
                printer_cstr.as_ptr() as *mut _,
                &mut h_printer,
                null_mut(),
            );

            if result == 0 || h_printer.is_null() {
                bail!("Failed to open printer '{}'", printer_name);
            }

            let _guard = PrinterGuard(h_printer);

            let doc_name = std::ffi::CString::new(
                file_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("document")
            )?;

            let mut doc_info = DOC_INFO_1A {
                pDocName: doc_name.as_ptr() as *mut _,
                pOutputFile: null_mut(),
                pDatatype: b"RAW\0".as_ptr() as *mut _, // RAW mode = no UI
            };

            let job_id = StartDocPrinterA(h_printer, 1, &mut doc_info as *mut _ as *mut _);
            if job_id == 0 {
                bail!("Failed to start print job");
            }

            if StartPagePrinter(h_printer) == 0 {
                EndDocPrinter(h_printer);
                bail!("Failed to start page");
            }

            let mut written: DWORD = 0;
            let write_result = WritePrinter(
                h_printer,
                file_content.as_ptr() as *mut _,
                file_content.len() as DWORD,
                &mut written,
            );

            EndPagePrinter(h_printer);
            EndDocPrinter(h_printer);

            if write_result == 0 {
                bail!("Failed to write to printer");
            }

            Ok(())
        }
    }
}

struct PrinterGuard(winapi::shared::ntdef::HANDLE);

impl Drop for PrinterGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                ClosePrinter(self.0);
            }
        }
    }
}

struct FileCleanup(PathBuf);

impl FileCleanup {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

impl Drop for FileCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

