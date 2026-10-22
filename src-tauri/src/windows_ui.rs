//! Regular app windows created on demand: settings, onboarding and the snapshot strip.

use crate::settings::Theme;
use crate::state::AppState;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

pub const STRIP: &str = "strip";
const STRIP_WIDTH: f64 = 132.0;
const STRIP_HEADER: f64 = 160.0;
const STRIP_ITEM: f64 = 92.0;
const STRIP_MAX_VISIBLE: f64 = 6.0;

/// Each strip update runs whole and applies the session state of its own moment, so a slow first build is
/// never overtaken by a later hide. Only taken on spawned threads.
static STRIP_LOCK: Mutex<()> = Mutex::new(());

pub fn open_settings(app: &AppHandle, tab: Option<&str>) {
    if let Some(win) = app.get_webview_window("settings") {
        if let Some(tab) = tab {
            let _ = app.emit_to("settings", "settings:tab", tab);
        }
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    let url = match tab {
        Some(tab) => format!("settings.html#{tab}"),
        None => "settings.html".to_string(),
    };
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App(url.into()))
        .title("WellSnap")
        .on_document_title_changed(follow_title)
        .theme(window_theme(app.state::<AppState>().settings().theme))
        .inner_size(760.0, 560.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .visible(true)
        .focused(true)
        .build();
    if let Err(err) = built {
        log::error!("could not open settings: {err}");
    }
}

pub fn open_onboarding(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(win) = app.get_webview_window("onboarding") {
            let _ = win.show();
            let _ = win.set_focus();
            return;
        }
        let built = WebviewWindowBuilder::new(&app, "onboarding", WebviewUrl::App("onboarding.html".into()))
            .title("WellSnap")
            .on_document_title_changed(follow_title)
            .theme(window_theme(app.state::<AppState>().settings().theme))
            .inner_size(640.0, 520.0)
            .resizable(false)
            .center()
            .focused(true)
            .build();
        if let Err(err) = built {
            log::error!("could not open onboarding: {err}");
        }
    });
}

/// The pages set a localized document.title; the taskbar and Alt+Tab show the native title.
fn follow_title(win: tauri::WebviewWindow, title: String) {
    if !title.is_empty() {
        let _ = win.set_title(&title);
    }
}

/// Native title bar and first paint in the app theme; None follows the system.
fn window_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    }
}

/// Applies a changed app theme to the open decorated windows (on macOS the theme is app-wide anyway).
pub fn apply_theme(app: &AppHandle, theme: Theme) {
    for label in ["settings", "onboarding"] {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.set_theme(window_theme(theme));
        }
    }
}

/// Shows or refreshes the strip; hides it when the set is empty.
pub fn sync_strip(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let _serial = STRIP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = app.state::<AppState>();
        let count = state.session.lock().unwrap().as_ref().map_or(0, |s| s.snapshots.len());
        let hide_from_capture = state.settings().hide_strip_from_capture;
        if count == 0 {
            if let Some(win) = app.get_webview_window(STRIP) {
                let _ = win.hide();
            }
            let _ = app.emit_to(STRIP, "session:changed", ());
            return;
        }
        let existing = app.get_webview_window(STRIP);
        // Re-dock only when the strip appears; a strip the user dragged elsewhere stays there.
        let appearing = existing.as_ref().is_none_or(|w| !w.is_visible().unwrap_or(false));
        let win = match existing {
            Some(win) => win,
            None => match WebviewWindowBuilder::new(&app, STRIP, WebviewUrl::App("strip.html".into()))
                .title("WellSnap")
                .decorations(false)
                .transparent(true)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .shadow(false)
                .focused(false)
                .visible(false)
                .content_protected(hide_from_capture)
                .inner_size(STRIP_WIDTH, STRIP_HEADER + STRIP_ITEM)
                .build()
            {
                Ok(win) => {
                    #[cfg(target_os = "macos")]
                    crate::macos::configure_floating(&win);
                    win
                }
                Err(err) => {
                    log::error!("could not create strip: {err}");
                    return;
                }
            },
        };
        let _ = win.set_content_protected(hide_from_capture);
        let height = STRIP_HEADER + STRIP_ITEM * (count as f64).min(STRIP_MAX_VISIBLE);
        let _ = win.set_size(LogicalSize::new(STRIP_WIDTH, height));
        if appearing {
            dock_right(&app, &win, (STRIP_WIDTH, height));
        }
        let _ = app.emit_to(STRIP, "session:changed", ());
        let _ = win.show();
    });
}

/// Destroys the hidden strip to free its webview; `sync_strip` creates it again. Called from the idle release
/// in overlay.rs, never on the main thread.
pub fn release_strip_if_hidden(app: &AppHandle) -> bool {
    let _serial = STRIP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match app.get_webview_window(STRIP) {
        Some(win) if !win.is_visible().unwrap_or(true) => win.destroy().is_ok(),
        _ => false,
    }
}

/// Right edge, vertically centered, of the monitor of the last capture. `size` is the strip's logical size.
fn dock_right(app: &AppHandle, win: &tauri::WebviewWindow, size: (f64, f64)) {
    let state = app.state::<AppState>();
    let monitor = {
        let ov = state.overlay.lock().unwrap();
        ov.last_monitor.clone().or_else(|| ov.monitors.first().cloned())
    }
    .or_else(|| crate::capture::monitors(app).into_iter().next());
    let Some(m) = monitor else { return };
    // macOS: global points, independent of the scale of the screen the strip is on now.
    #[cfg(target_os = "macos")]
    {
        let (x, y) = right_middle(crate::capture::monitor_points(&m), size, 1.0);
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = size;
        let Ok(outer) = win.outer_size() else { return };
        let rect = [f64::from(m.x), f64::from(m.y), f64::from(m.width), f64::from(m.height)];
        let (x, y) = right_middle(rect, (f64::from(outer.width), f64::from(outer.height)), m.scale);
        let _ = win.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    }
}

/// 16 points from the right edge, centered vertically; `scale` converts the margin into the rect's units.
fn right_middle([mx, my, mw, mh]: [f64; 4], (w, h): (f64, f64), scale: f64) -> (f64, f64) {
    (mx + mw - w - (16.0 * scale).trunc(), my + ((mh - h) / 2.0).trunc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_docks_right_in_physical_pixels() {
        assert_eq!(right_middle([1920.0, 0.0, 2560.0, 1440.0], (264.0, 448.0), 2.0), (4184.0, 496.0));
    }

    #[test]
    fn strip_docks_on_a_1x_external_monitor_in_points() {
        let m = crate::capture::MonitorInfo {
            id: "1".into(),
            name: "ext".into(),
            x: 1512,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        };
        let (x, y) = right_middle(crate::capture::monitor_points(&m), (STRIP_WIDTH, 316.0), 1.0);
        assert_eq!((x, y), (1512.0 + 1920.0 - STRIP_WIDTH - 16.0, 382.0));
    }
}
