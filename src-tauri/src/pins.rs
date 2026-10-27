//! Pinned screenshots: a snapshot in its own small window above all others, to compare it with what is under
//! it (src/pin). A pin from the editor opens over the area it was taken from, one image pixel per screen pixel;
//! a pin from the strip opens in the middle of the monitor under the cursor. The page moves, zooms and fades
//! its own window through the commands here. Pins live until closed and are not restored after a restart.

use crate::capture::MonitorInfo;
use crate::i18n::tr;
use crate::state::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Each pin is a webview, so their number is capped.
pub const MAX_PINS: usize = 20;
/// Window side limits in physical pixels; the page keeps the zoom within 20% and 500% anyway.
const MIN_SIDE: u32 = 16;
const MAX_SIDE: u32 = 16384;
const LABEL_PREFIX: &str = "pin-";

#[derive(Default)]
pub struct Pins {
    next: u64,
    images: HashMap<u64, Pin>,
}

struct Pin {
    png: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

impl Pins {
    pub fn count(&self) -> usize {
        self.images.len()
    }

    /// A new id for this image, or None when `MAX_PINS` are open.
    fn add(&mut self, png: Vec<u8>, width: u32, height: u32) -> Option<u64> {
        if self.images.len() >= MAX_PINS {
            return None;
        }
        self.next += 1;
        self.images.insert(self.next, Pin { png: Arc::new(png), width, height });
        Some(self.next)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinInfo {
    pub id: u64,
    /// Image size in pixels.
    pub width: u32,
    pub height: u32,
}

pub fn is_pin_label(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

fn id_of(label: &str) -> Option<u64> {
    label.strip_prefix(LABEL_PREFIX)?.parse().ok()
}

/// The image of pin `id`, for the `wsnap` protocol and the copy and save commands.
pub fn png(app: &AppHandle, id: u64) -> Option<Arc<Vec<u8>>> {
    app.state::<AppState>().pins.lock().unwrap().images.get(&id).map(|p| p.png.clone())
}

fn png_of(win: &WebviewWindow) -> Result<Arc<Vec<u8>>, String> {
    id_of(win.label()).and_then(|id| png(win.app_handle(), id)).ok_or_else(|| "this pin is closed".to_string())
}

pub fn open_count(app: &AppHandle) -> usize {
    app.state::<AppState>().pins.lock().unwrap().count()
}

/// Opens a pin of `png` on `monitor`: over the area at `offset` (physical pixels from the monitor's top-left
/// corner), or centered when there is none. Not on the main thread: building a webview there would deadlock.
pub fn open(
    app: &AppHandle,
    png: Vec<u8>,
    (width, height): (u32, u32),
    monitor: Option<MonitorInfo>,
    offset: Option<(i32, i32)>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(id) = state.pins.lock().unwrap().add(png, width, height) else {
        return Err(tr(state.lang(), "toast.pinsFull").replace("{max}", &MAX_PINS.to_string()));
    };
    let label = format!("{LABEL_PREFIX}{id}");
    // Shown by the page once its image is decoded (`show`), so it never flashes empty.
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("pin.html".into()))
        .title("WellSnap")
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build();
    let win = match built {
        Ok(win) => win,
        Err(err) => {
            state.pins.lock().unwrap().images.remove(&id);
            log::error!("could not open a pin: {err}");
            return Err(err.to_string());
        }
    };
    #[cfg(target_os = "macos")]
    crate::macos::configure_floating(&win);
    let monitor = monitor.or_else(|| crate::overlay::monitor_under_cursor(app));
    match monitor {
        Some(m) => place(&win, &m, (width, height), offset),
        None => {
            let _ = win.set_size(tauri::PhysicalSize::new(width.max(MIN_SIDE), height.max(MIN_SIDE)));
            let _ = win.center();
        }
    }
    log::info!("pin {id} opened ({width}x{height})");
    crate::tray::refresh(app);
    Ok(())
}

fn place(win: &WebviewWindow, m: &MonitorInfo, size: (u32, u32), offset: Option<(i32, i32)>) {
    let points = cfg!(target_os = "macos");
    let [x, y, w, h] = frame(m, size, offset, points);
    if points {
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
    } else {
        // Sized before it moves too: the default 800x600 window at the area could reach over onto a monitor
        // with another DPI, and the system would rescale it there. Sized again after the move, which may have
        // rescaled it anyway.
        let size = tauri::PhysicalSize::new(w.round() as u32, h.round() as u32);
        let _ = win.set_size(size);
        let _ = win.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
        let _ = win.set_size(size);
    }
}

/// Window rect for an image of `size` physical pixels on monitor `m`: at `offset` (physical pixels within the
/// monitor) or centered. In global points when `points` (macOS, where each monitor has its own scale), physical
/// pixels otherwise.
fn frame(m: &MonitorInfo, (width, height): (u32, u32), offset: Option<(i32, i32)>, points: bool) -> [f64; 4] {
    let scale = if m.scale.is_finite() && m.scale > 0.0 { m.scale } else { 1.0 };
    let unit = if points { scale } else { 1.0 };
    let [mx, my, mw, mh] = [f64::from(m.x), f64::from(m.y), f64::from(m.width), f64::from(m.height)].map(|v| v / unit);
    let (w, h) = (f64::from(width.max(MIN_SIDE)) / unit, f64::from(height.max(MIN_SIDE)) / unit);
    let (x, y) = match offset {
        Some((ox, oy)) => (mx + f64::from(ox) / unit, my + f64::from(oy) / unit),
        None => (mx + ((mw - w) / 2.0).max(0.0).trunc(), my + ((mh - h) / 2.0).max(0.0).trunc()),
    };
    [x, y, w, h]
}

pub fn info(win: &WebviewWindow) -> Result<PinInfo, String> {
    let id = id_of(win.label()).ok_or("not a pin")?;
    let state = win.state::<AppState>();
    let pins = state.pins.lock().unwrap();
    let pin = pins.images.get(&id).ok_or("this pin is closed")?;
    Ok(PinInfo { id, width: pin.width, height: pin.height })
}

/// The page has decoded its image: shows the window without taking focus from the app the user works in.
pub fn show(win: &WebviewWindow) {
    let _ = win.show();
}

/// Resizes the window to `width` x `height` physical pixels, keeping the point at `anchor` (fractions of the
/// window, the cursor for a wheel zoom) where it is on screen.
pub fn resize(win: &WebviewWindow, width: u32, height: u32, anchor: (f64, f64)) {
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else { return };
    let new = fit((width, height));
    let (x, y) = anchored((pos.x, pos.y), (size.width, size.height), new, anchor);
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = win.set_size(tauri::PhysicalSize::new(new.0, new.1));
}

/// `size` within the side limits, scaled as a whole so the image keeps its proportions.
fn fit((width, height): (u32, u32)) -> (u32, u32) {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let (long, short) = (w.max(h), w.min(h));
    // Grown until the short side is visible, but the long side's limit wins for a very thin image.
    let k = (f64::from(MIN_SIDE) / short).max(1.0).min(f64::from(MAX_SIDE) / long);
    (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1))
}

/// Top-left corner after a resize from `size` to `new` that keeps the point at `anchor` in place.
fn anchored(pos: (i32, i32), size: (u32, u32), new: (u32, u32), (ax, ay): (f64, f64)) -> (i32, i32) {
    let ax = if ax.is_finite() { ax.clamp(0.0, 1.0) } else { 0.0 };
    let ay = if ay.is_finite() { ay.clamp(0.0, 1.0) } else { 0.0 };
    let shift = |p: i32, old: u32, new: u32, a: f64| p + ((f64::from(old) - f64::from(new)) * a).round() as i32;
    (shift(pos.0, size.0, new.0, ax), shift(pos.1, size.1, new.1, ay))
}

/// Moves the window by `dx`, `dy` logical pixels (the arrow keys).
pub fn nudge(win: &WebviewWindow, dx: i32, dy: i32) {
    let (Ok(pos), Ok(scale)) = (win.outer_position(), win.scale_factor()) else { return };
    let step = |d: i32| (f64::from(d.clamp(-100, 100)) * scale).round() as i32;
    let _ = win.set_position(tauri::PhysicalPosition::new(pos.x + step(dx), pos.y + step(dy)));
}

/// macOS fades the whole window natively and returns true; elsewhere the page fades its image itself.
pub fn set_opacity(win: &WebviewWindow, opacity: f64) -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::macos::set_alpha(win, opacity.clamp(0.1, 1.0));
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (win, opacity);
        false
    }
}

pub fn copy(win: &WebviewWindow) -> Result<(), String> {
    let png = png_of(win)?;
    crate::clipboard::copy_image(png.as_ref().clone(), None).map_err(|e| e.to_string())?;
    let app = win.app_handle();
    crate::notice::toast(app, tr(app.state::<AppState>().lang(), "toast.copied"));
    Ok(())
}

/// Always PNG, like "Save all" of the set: the pin keeps no JPEG of its own.
pub fn save(win: &WebviewWindow) -> Result<(), String> {
    let png = png_of(win)?;
    let app = win.app_handle();
    let dir = crate::actions::save_dir(app);
    let path = crate::export::save_unique(&dir, &crate::export::timestamp_stem(chrono::Local::now()), "png", &png)
        .map_err(|e| e.to_string())?;
    crate::notice::toast_with_detail(app, tr(app.state::<AppState>().lang(), "toast.saved"), &path.to_string_lossy());
    Ok(())
}

/// Every pin window, for "Close all".
pub fn close_all(app: &AppHandle) {
    for (label, win) in app.webview_windows() {
        if is_pin_label(&label) {
            let _ = win.destroy();
        }
    }
}

/// A pin window is gone (closed by the page, the tray, or the system): its image is dropped.
pub fn closed(app: &AppHandle, label: &str) {
    let Some(id) = id_of(label) else { return };
    let removed = app.state::<AppState>().pins.lock().unwrap().images.remove(&id).is_some();
    if removed {
        log::info!("pin {id} closed");
        crate::tray::refresh(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: i32, y: i32, scale: f64) -> MonitorInfo {
        MonitorInfo { id: "0".into(), name: "m".into(), x, y, width: 2880, height: 1800, scale }
    }

    #[test]
    fn a_pin_opens_over_its_area_one_to_one() {
        let m = monitor(1920, 0, 2.0);
        assert_eq!(frame(&m, (400, 300), Some((100, 50)), false), [2020.0, 50.0, 400.0, 300.0]);
        // macOS: global points, the image keeps its physical size on the Retina screen.
        assert_eq!(frame(&m, (400, 300), Some((100, 50)), true), [1010.0, 25.0, 200.0, 150.0]);
    }

    #[test]
    fn a_pin_without_an_area_is_centered_and_never_tiny() {
        let m = monitor(0, 0, 1.0);
        assert_eq!(frame(&m, (880, 800), None, false), [1000.0, 500.0, 880.0, 800.0]);
        assert_eq!(frame(&m, (4, 2), None, false)[2..], [16.0, 16.0]);
        // Larger than the monitor: from its corner, not off screen.
        assert_eq!(frame(&m, (4000, 2000), None, false)[..2], [0.0, 0.0]);
    }

    #[test]
    fn a_zoom_keeps_the_anchor_in_place() {
        // Doubling around the middle grows the window on every side.
        assert_eq!(anchored((100, 100), (200, 100), (400, 200), (0.5, 0.5)), (0, 50));
        // Around the top-left corner it stays put.
        assert_eq!(anchored((100, 100), (200, 100), (400, 200), (0.0, 0.0)), (100, 100));
        // Shrinking around the bottom-right corner moves the corner in.
        assert_eq!(anchored((100, 100), (200, 100), (100, 50), (1.0, 1.0)), (200, 150));
        assert_eq!(anchored((100, 100), (200, 100), (100, 50), (f64::NAN, 7.0)), (100, 150));
    }

    #[test]
    fn the_window_size_keeps_the_image_proportions_within_the_limits() {
        assert_eq!(fit((800, 600)), (800, 600));
        // A 4K screen at 500%.
        assert_eq!(fit((19200, 10800)), (16384, 9216));
        assert_eq!(fit((8, 4)), (32, 16));
        // Too thin for both limits: the long side wins.
        assert_eq!(fit((40000, 2)), (16384, 1));
    }

    #[test]
    fn pins_are_capped_and_numbered() {
        let mut pins = Pins::default();
        let first = pins.add(vec![1], 10, 10).unwrap();
        for _ in 1..MAX_PINS {
            pins.add(vec![1], 10, 10).unwrap();
        }
        assert_eq!(pins.add(vec![1], 10, 10), None, "the cap");
        pins.images.remove(&first);
        assert_eq!(pins.add(vec![1], 10, 10), Some(MAX_PINS as u64 + 1), "ids are never reused");
        assert_eq!(pins.count(), MAX_PINS);
    }

    #[test]
    fn pin_labels() {
        assert!(is_pin_label("pin-3") && !is_pin_label("strip"));
        assert_eq!(id_of("pin-12"), Some(12));
        assert_eq!(id_of("pin-x"), None);
        assert_eq!(id_of("overlay-1"), None);
    }
}
