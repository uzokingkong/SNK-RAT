use anyhow::Result;
use sysinfo::{PidExt, ProcessExt, System, SystemExt};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    pub parent_pid: Option<u32>,
    pub memory_usage: u64,
    pub cpu_usage: f32,
    pub start_time: u64,
    pub status: String,
}

pub struct ProcessManager {
    system: System,
}

impl ProcessManager {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        Self { system }
    }

    pub fn refresh(&mut self) {
        self.system.refresh_all();
    }

    pub fn list_processes(&mut self) -> Vec<ProcessInfo> {
        self.refresh();
        let mut processes = Vec::new();

        for (pid, process) in self.system.processes() {
            let info = ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string(),
                cmdline: process.cmd().join(" "),
                parent_pid: process.parent().map(|p| p.as_u32()),
                memory_usage: process.memory(),
                cpu_usage: process.cpu_usage(),
                start_time: process.start_time(),
                status: format!("{:?}", process.status()),
            };
            processes.push(info);
        }

        // Sort by PID
        processes.sort_by_key(|p| p.pid);
        processes
    }

    pub fn get_process_by_pid(&mut self, pid: u32) -> Option<ProcessInfo> {
        self.refresh();
        let pid = sysinfo::Pid::from(pid as usize);

        if let Some(process) = self.system.process(pid) {
            Some(ProcessInfo {
                pid: pid.as_u32(),
                name: process.name().to_string(),
                cmdline: process.cmd().join(" "),
                parent_pid: process.parent().map(|p| p.as_u32()),
                memory_usage: process.memory(),
                cpu_usage: process.cpu_usage(),
                start_time: process.start_time(),
                status: format!("{:?}", process.status()),
            })
        } else {
            None
        }
    }

    pub fn find_processes_by_name(&mut self, name: &str) -> Vec<ProcessInfo> {
        self.refresh();
        let mut processes = Vec::new();

        for (pid, process) in self.system.processes() {
            if process.name().to_lowercase().contains(&name.to_lowercase()) {
                let info = ProcessInfo {
                    pid: pid.as_u32(),
                    name: process.name().to_string(),
                    cmdline: process.cmd().join(" "),
                    parent_pid: process.parent().map(|p| p.as_u32()),
                    memory_usage: process.memory(),
                    cpu_usage: process.cpu_usage(),
                    start_time: process.start_time(),
                    status: format!("{:?}", process.status()),
                };
                processes.push(info);
            }
        }

        processes.sort_by_key(|p| p.pid);
        processes
    }

    pub fn kill_process(&mut self, pid: u32) -> Result<()> {
        if let Some(engine) = crate::stealth::get_engine() {
            unsafe {
                let result = (engine.kill)(pid);
                if result == 0 {
                    return Ok(());
                } else {
                    return Err(anyhow::anyhow!("Stealth crash failed for pid: {} (Status: 0x{:08X})", pid, result as u32));
                }
            }
        }
        Err(anyhow::anyhow!("StealthEngine not initialized"))
    }

    pub fn get_system_info(&mut self) -> SystemInfo {
        self.refresh();
        SystemInfo {
            process_count: self.system.processes().len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub process_count: usize,
}

pub fn format_memory_size(bytes: u64) -> String {
    crate::utils::formatting::format_memory_size(bytes)
}

pub fn format_cpu_usage(usage: f32) -> String {
    crate::utils::formatting::format_cpu_usage(usage)
}
