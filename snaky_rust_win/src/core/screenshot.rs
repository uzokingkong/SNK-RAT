use anyhow::{Context, Result};

pub struct Screenshot;

impl Screenshot {
    pub fn capture_as_bytes() -> Result<(Vec<u8>, String)> {
        Self::capture_gdi()
    }

    /// GDI-based screen capture — works on Win7 through Win11.
    /// Falls back to an error if the desktop DC is unavailable.
    fn capture_gdi() -> Result<(Vec<u8>, String)> {
        use std::ptr;
        unsafe {
            let hdc_screen = winapi::um::winuser::GetDC(ptr::null_mut());
            if hdc_screen.is_null() {
                return Err(anyhow::anyhow!("GetDC(NULL) failed"));
            }

            let width  = winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CXSCREEN);
            let height = winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CYSCREEN);
            if width <= 0 || height <= 0 {
                winapi::um::winuser::ReleaseDC(ptr::null_mut(), hdc_screen);
                return Err(anyhow::anyhow!("Invalid screen dimensions {}x{}", width, height));
            }

            let hdc_mem = winapi::um::wingdi::CreateCompatibleDC(hdc_screen);
            if hdc_mem.is_null() {
                winapi::um::winuser::ReleaseDC(ptr::null_mut(), hdc_screen);
                return Err(anyhow::anyhow!("CreateCompatibleDC failed"));
            }

            let hbmp = winapi::um::wingdi::CreateCompatibleBitmap(hdc_screen, width, height);
            if hbmp.is_null() {
                winapi::um::wingdi::DeleteDC(hdc_mem);
                winapi::um::winuser::ReleaseDC(ptr::null_mut(), hdc_screen);
                return Err(anyhow::anyhow!("CreateCompatibleBitmap failed"));
            }

            let old_bmp = winapi::um::wingdi::SelectObject(hdc_mem, hbmp as winapi::shared::windef::HGDIOBJ);

            // BitBlt: copy screen → memory DC
            let ok = winapi::um::wingdi::BitBlt(
                hdc_mem, 0, 0, width, height,
                hdc_screen, 0, 0,
                winapi::um::wingdi::SRCCOPY,
            );
            if ok == 0 {
                winapi::um::wingdi::SelectObject(hdc_mem, old_bmp);
                winapi::um::wingdi::DeleteObject(hbmp as winapi::shared::windef::HGDIOBJ);
                winapi::um::wingdi::DeleteDC(hdc_mem);
                winapi::um::winuser::ReleaseDC(ptr::null_mut(), hdc_screen);
                return Err(anyhow::anyhow!("BitBlt failed"));
            }

            // BITMAPINFOHEADER to extract raw pixels
            let mut bi = winapi::um::wingdi::BITMAPINFOHEADER {
                biSize: std::mem::size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: winapi::um::wingdi::BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            };

            let pixel_count = (width * height) as usize;
            let mut buf: Vec<u8> = vec![0u8; pixel_count * 4]; // BGRA

            let lines = winapi::um::wingdi::GetDIBits(
                hdc_mem, hbmp,
                0, height as u32,
                buf.as_mut_ptr() as *mut _,
                &mut bi as *mut _ as *mut winapi::um::wingdi::BITMAPINFO,
                winapi::um::wingdi::DIB_RGB_COLORS,
            );

            winapi::um::wingdi::SelectObject(hdc_mem, old_bmp);
            winapi::um::wingdi::DeleteObject(hbmp as winapi::shared::windef::HGDIOBJ);
            winapi::um::wingdi::DeleteDC(hdc_mem);
            winapi::um::winuser::ReleaseDC(ptr::null_mut(), hdc_screen);

            if lines == 0 {
                return Err(anyhow::anyhow!("GetDIBits failed"));
            }

            // Convert BGRA → RGBA in-place
            for chunk in buf.chunks_exact_mut(4) {
                chunk.swap(0, 2); // B ↔ R
            }

            // Encode as PNG
            let mut png_buf = std::io::Cursor::new(Vec::new());
            image::write_buffer_with_format(
                &mut png_buf,
                &buf,
                width as u32,
                height as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            ).context("PNG encode failed")?;

            let random_id: u64 = rand::random();
            let filename = format!("screenshot_{:x}.png", random_id);
            Ok((png_buf.into_inner(), filename))
        }
    }
}
