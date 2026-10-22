//! GDI capture of one monitor rectangle. The process is per-monitor DPI aware (v2, set by tao), so the
//! screen DC is in physical pixels. CAPTUREBLT includes layered windows (tooltips, context menus), as ShareX
//! does; without it they can be missing from the frozen frame.

use super::{CaptureError, MonitorInfo};
use std::ffi::c_void;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};

pub fn capture_bgra(m: &MonitorInfo) -> Result<(u32, u32, Vec<u8>), CaptureError> {
    let (w, h) = (m.width as i32, m.height as i32);
    if w <= 0 || h <= 0 {
        return Err(CaptureError::Failed(format!("empty monitor rect {w}x{h}")));
    }
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err(CaptureError::Failed("GetDC failed".into()));
        }
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(dib) => {
                let old = SelectObject(mem, HGDIOBJ(dib.0));
                let blit = BitBlt(mem, 0, 0, w, h, Some(screen), m.x, m.y, SRCCOPY | CAPTUREBLT);
                let out = match blit {
                    Ok(()) if !bits.is_null() => {
                        let len = (w as usize) * (h as usize) * 4;
                        Ok((m.width, m.height, std::slice::from_raw_parts(bits as *const u8, len).to_vec()))
                    }
                    Ok(()) => Err(CaptureError::Failed("DIB section has no bits".into())),
                    Err(err) => Err(CaptureError::Failed(format!("BitBlt: {err}"))),
                };
                SelectObject(mem, old);
                let _ = DeleteObject(HGDIOBJ(dib.0));
                out
            }
            Err(err) => Err(CaptureError::Failed(format!("CreateDIBSection: {err}"))),
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}
