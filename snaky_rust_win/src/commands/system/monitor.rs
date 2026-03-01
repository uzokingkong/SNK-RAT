use crate::commands::Arguments;
use crate::commands::BotCommand;
use crate::core::process::{format_cpu_usage, format_memory_size, ProcessManager};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use sysinfo::{CpuExt, DiskExt, PidExt, ProcessExt, System, SystemExt};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::embed::EmbedField;
use twilight_model::channel::message::Message;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFooterBuilder};
use crate::core::stego_store::{StegoStore, StringCategory};

pub struct MonitorCommand;

impl MonitorCommand {
    fn s(key: &str) -> String { StegoStore::get(StringCategory::System, key) }
}

#[async_trait]
impl BotCommand for MonitorCommand {
    fn name(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "MONITOR_NAME").into_boxed_str())
    }
    
    fn description(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::Desc, "MONITOR").into_boxed_str())
    }
    
    fn category(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "CAT_SYSTEM").into_boxed_str())
    }
    
    fn usage(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::CmdMeta, "MONITOR_USAGE").into_boxed_str())
    }
    
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(format!("{} {}", 
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_USAGE").split_whitespace().next().unwrap_or(".monitor"),
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_CPU")).into_boxed_str()) as &'static str,
            Box::leak(format!("{} {}", 
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_USAGE").split_whitespace().next().unwrap_or(".monitor"),
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_MEM")).into_boxed_str()) as &'static str,
            Box::leak(format!("{} {}", 
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_USAGE").split_whitespace().next().unwrap_or(".monitor"),
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_DISK")).into_boxed_str()) as &'static str,
            Box::leak(format!("{} {}", 
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_USAGE").split_whitespace().next().unwrap_or(".monitor"),
                StegoStore::get(StringCategory::CmdMeta, "MONITOR_PROCS")).into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }
    
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "MON_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(StegoStore::get(StringCategory::CmdMeta, "MON_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let resource = match args.next() {
            Some(resource) => resource,
            None => {
                return self.show_all_resources(http, msg).await;
            }
        };

        match resource {
            "cpu" => self.monitor_cpu(http, msg).await,
            "memory" | "mem" => self.monitor_memory(http, msg).await,
            "disk" => self.monitor_disk(http, msg).await,
            "processes" | "procs" => self.monitor_processes(http, msg).await,
            _ => {
                let embed = EmbedBuilder::new()
                    .title(Self::s("MONITOR_TITLE"))
                    .description(Self::s("LBL_MON_RES"))
                    .color(0xFF6B6B)
                    .field(EmbedField {
                        name: Self::s("LBL_USAGE_LBL"),
                        value: Self::s("MONITOR_USAGE"),
                        inline: false,
                    })
                    .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
                    .build();

                http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
                Ok(())
            }
        }
    }
}

impl MonitorCommand {
    async fn show_all_resources(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let mut system = System::new_all();
        system.refresh_all();

        // CPU Info
        let cpu_usage = system.global_cpu_info().cpu_usage();
        let cpu_fields = vec![
            EmbedField {
                name: Self::s("LBL_CPU_USAGE"),
                value: format!("{:.1}%", cpu_usage),
                inline: true,
            },
        ];

        let cpu_embed = EmbedBuilder::new()
            .title(Self::s("CPU_MON"))
            .color(0xFFA500)
            .field(cpu_fields[0].clone())
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.get(), &[cpu_embed]).await?;

        // Memory Info
        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available_memory = total_memory - used_memory;
        let memory_usage_percent = (used_memory as f64 / total_memory as f64) * 100.0;

        let memory_embed = EmbedBuilder::new()
            .title(Self::s("MEM_MON"))
            .color(0x4169E1)
            .field(EmbedField {
                name: Self::s("LBL_USED_MEM"),
                value: format!("{} ({:.1}%)", self.format_bytes(used_memory), memory_usage_percent),
                inline: true,
            })
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.get(), &[memory_embed]).await?;

        // Disk Info
        let disks: Vec<_> = system.disks().iter().collect();
        let mut disk_fields = Vec::new();

        for (i, disk) in disks.iter().take(5).enumerate() {
            let total_space = disk.total_space();
            let available_space = disk.available_space();
            let used_space = total_space - available_space;
            
            let mount_point = disk.mount_point().to_string_lossy();
            let disk_name = disk.name().to_string_lossy();
            let name = if disk_name.is_empty() { Self::s("UNNAMED") } else { disk_name.to_string() };

            let disk_title = Self::s("LBL_DISK_ITEM")
                .replace("{num}", &(i + 1).to_string())
                .replace("{name}", &name);
            
            let mount_val = Self::s("LBL_MOUNT_LBL").replace("{mount}", &mount_point);
            let size_val = Self::s("LBL_SIZE_LBL").replace("{size}", &self.format_bytes(total_space));
            let used_val = Self::s("LBL_USED_LBL").replace("{used}", &self.format_bytes(used_space));

            disk_fields.push(EmbedField {
                name: disk_title,
                value: format!("{}\n{}\n{}", mount_val, size_val, used_val),
                inline: false,
            });
        }

        let mut disk_embed = EmbedBuilder::new().title(Self::s("DISK_MON")).color(0x32CD32);
        for field in disk_fields { disk_embed = disk_embed.field(field); }
        let disk_embed = disk_embed.footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER"))).build();

        http.create_message_with_embeds(msg.channel_id.get(), &[disk_embed]).await?;

        // Processes Info
        let process_count = system.processes().len();
        let process_embed = EmbedBuilder::new()
            .title(Self::s("PROC_MON"))
            .color(0x9370DB)
            .field(EmbedField {
                name: Self::s("TOTAL_PROC"),
                value: process_count.to_string(),
                inline: true,
            })
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();

        http.create_message_with_embeds(msg.channel_id.get(), &[process_embed]).await?;

        Ok(())
    }

    async fn monitor_cpu(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let mut system = System::new_all();
        system.refresh_all();
        let cpu_usage = system.global_cpu_info().cpu_usage();
        let embed = EmbedBuilder::new()
            .title(Self::s("CPU_MON"))
            .color(0xFFA500)
            .field(EmbedField { name: Self::s("LBL_USAGE_LBL"), value: format!("{:.1}%", cpu_usage), inline: true })
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();
        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        Ok(())
    }

    async fn monitor_memory(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let mut system = System::new_all();
        system.refresh_all();
        let used = system.used_memory();
        let total = system.total_memory();
        let embed = EmbedBuilder::new()
            .title(Self::s("MEM_MON"))
            .color(0x4169E1)
            .field(EmbedField { name: Self::s("LBL_USAGE_LBL"), value: format!("{}/{}", self.format_bytes(used), self.format_bytes(total)), inline: true })
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();
        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        Ok(())
    }

    async fn monitor_disk(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        self.show_all_resources(http, msg).await // reused logic
    }

    async fn monitor_processes(&self, http: &Arc<HttpClient>, msg: &Message) -> Result<()> {
        let mut system = System::new_all();
        system.refresh_all();
        let embed = EmbedBuilder::new()
            .title(Self::s("PROC_MON"))
            .color(0x9370DB)
            .field(EmbedField { name: Self::s("TOTAL_PROC"), value: system.processes().len().to_string(), inline: true })
            .footer(EmbedFooterBuilder::new(Self::s("PROC_MANAGER")))
            .build();
        http.create_message_with_embeds(msg.channel_id.get(), &[embed]).await?;
        Ok(())
    }

    fn format_bytes(&self, bytes: u64) -> String {
        crate::utils::formatting::format_memory_size(bytes)
    }
}
