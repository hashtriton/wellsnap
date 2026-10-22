//! Screen capture of whole monitors, done before any overlay is shown (otherwise the overlay itself would
//! end up in the frame). Frames are tightly packed, top-down, opaque RGBA, so the webview can wrap them in
//! ImageData without another pass: GDI gives BGRA, converted here; the macOS backend converts while it copies.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// Stable key for this monitor during the app session (index-based label suffix).
    pub id: String,
    pub name: String,
    /// Physical pixels in the virtual desktop.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA, 4 bytes per pixel, row stride = width * 4, alpha = 255.
    pub rgba: Vec<u8>,
    pub capture_ms: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    #[error("screen recording permission is not granted")]
    PermissionDenied,
    #[error("capture failed: {0}")]
    Failed(String),
}

pub fn monitors(app: &tauri::AppHandle) -> Vec<MonitorInfo> {
    let list = app.available_monitors().unwrap_or_default();
    list.iter()
        .enumerate()
        .map(|(i, m)| MonitorInfo {
            id: i.to_string(),
            name: m.name().cloned().unwrap_or_default(),
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
            scale: m.scale_factor(),
        })
        .collect()
}

/// (x, y, width, height) of the monitor in global display points: on macOS tao reports each monitor's origin
/// and size in points times that monitor's own scale, so windows on screens with different scales must be
/// placed in points. Used by the macOS window placement and the macOS capture backend.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn monitor_points(m: &MonitorInfo) -> [f64; 4] {
    let scale = if m.scale.is_finite() && m.scale > 0.0 { m.scale } else { 1.0 };
    [f64::from(m.x) / scale, f64::from(m.y) / scale, f64::from(m.width) / scale, f64::from(m.height) / scale]
}

/// Captures every monitor in parallel. Results keep the order of `monitors`.
pub fn capture_all(monitors: &[MonitorInfo]) -> Vec<Result<Frame, CaptureError>> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = monitors.iter().map(|m| scope.spawn(move || capture_monitor(m))).collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err(CaptureError::Failed("capture thread panicked".into()))))
            .collect()
    })
}

pub fn capture_monitor(m: &MonitorInfo) -> Result<Frame, CaptureError> {
    let started = Instant::now();
    #[cfg(windows)]
    let (width, height, pixels) = {
        let (width, height, mut pixels) = windows::capture_bgra(m)?;
        bgra_to_rgba_opaque(&mut pixels);
        (width, height, pixels)
    };
    #[cfg(target_os = "macos")]
    let (width, height, pixels) = macos::capture_rgba(m)?;
    #[cfg(not(any(windows, target_os = "macos")))]
    let (width, height, pixels): (u32, u32, Vec<u8>) = return Err(CaptureError::Failed("unsupported platform".into()));

    Ok(Frame { width, height, rgba: pixels, capture_ms: started.elapsed().as_secs_f64() * 1000.0 })
}

#[cfg(target_os = "macos")]
pub use macos::{has_permission, prewarm, request_permission};

#[cfg(not(target_os = "macos"))]
pub fn has_permission() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn request_permission() -> bool {
    true
}

/// Swaps B and R and forces alpha to 255 (GDI leaves alpha undefined). Split across threads: a 4K frame is
/// 33 MB and this sits on the hotkey-to-overlay path.
pub fn bgra_to_rgba_opaque(buf: &mut [u8]) {
    const MIN_CHUNK: usize = 1 << 20;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let chunk = (buf.len() / threads).max(MIN_CHUNK).next_multiple_of(4);
    std::thread::scope(|scope| {
        for part in buf.chunks_mut(chunk) {
            scope.spawn(move || {
                for px in part.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                    px[3] = 255;
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: i32, y: i32, width: u32, height: u32, scale: f64) -> MonitorInfo {
        MonitorInfo { id: String::new(), name: String::new(), x, y, width, height, scale }
    }

    #[test]
    fn monitor_points_on_a_mixed_scale_layout() {
        // 2x MacBook (1512x982 pt) as the main screen, a 1x 1080p monitor to its right at 1512 pt.
        assert_eq!(monitor_points(&monitor(0, 0, 3024, 1964, 2.0)), [0.0, 0.0, 1512.0, 982.0]);
        assert_eq!(monitor_points(&monitor(1512, 0, 1920, 1080, 1.0)), [1512.0, 0.0, 1920.0, 1080.0]);
        // 1x main, Retina external at 1920 pt: tao reports x = 3840.
        assert_eq!(monitor_points(&monitor(3840, -200, 3024, 1964, 2.0)), [1920.0, -100.0, 1512.0, 982.0]);
    }

    #[test]
    fn monitor_points_treat_a_bad_scale_as_1x() {
        for scale in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(monitor_points(&monitor(10, 20, 30, 40, scale)), [10.0, 20.0, 30.0, 40.0]);
        }
    }

    #[test]
    fn swizzle_and_opaque() {

        let mut px = vec![1, 2, 3, 0, 10, 20, 30, 7];
        bgra_to_rgba_opaque(&mut px);
        assert_eq!(px, vec![3, 2, 1, 255, 30, 20, 10, 255]);
    }

    #[test]
    fn swizzle_large_buffer_crosses_chunks() {
        let n = 3 * (1 << 20) + 12;
        let mut px: Vec<u8> = (0..n).map(|i| (i % 4) as u8).collect();
        bgra_to_rgba_opaque(&mut px);
        assert!(px.as_chunks::<4>().0.iter().all(|p| *p == [2, 1, 0, 255]));
    }
}
