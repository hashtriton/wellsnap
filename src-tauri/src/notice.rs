//! Small always-on-top notice window: click-through toasts ("Copied"), the donation reminder card and the
//! clickable "updated" pill shown after an automatic update.
//! In-app instead of system notifications, which depend on OS permissions and Start Menu shortcuts.

use crate::state::AppState;
use serde::Serialize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "notice";
const TOAST_SIZE: (f64, f64) = (420.0, 64.0);
const CARD_SIZE: (f64, f64) = (400.0, 236.0);
const UPDATED_SIZE: (f64, f64) = (420.0, 88.0);
const TOAST_MS: u64 = 2600;
const ERROR_TOAST_MS: u64 = 6000;

static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Serializes get-or-create and show across the threads `show` spawns, so two early notices do not both
/// build the window and notices appear in the order they were requested. Never taken on the main thread.
static NOTICE_LOCK: Mutex<()> = Mutex::new(());
/// The notice on screen, for a page that loads after it was sent (`notice_ready`).
static PENDING: Mutex<Option<NoticePayload>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NoticePayload {
    Toast { text: String, detail: Option<String>, error: bool },
    /// Donation reminder; `index` is 0..4 and picks the copy on the frontend.
    Nudge { index: u32, captures: u32 },
    /// WellSnap restarted into `version`; the pill hides itself on the frontend.
    #[serde(rename_all = "camelCase")]
    Updated { version: String, has_notes: bool, set_cleared: bool },
}

pub fn toast(app: &AppHandle, text: &str) {
    show(app, NoticePayload::Toast { text: text.to_string(), detail: None, error: false });
}

pub fn toast_with_detail(app: &AppHandle, text: &str, detail: &str) {
    show(app, NoticePayload::Toast { text: text.to_string(), detail: Some(detail.to_string()), error: false });
}

pub fn toast_error(app: &AppHandle, text: &str, detail: Option<&str>) {
    show(app, NoticePayload::Toast { text: text.to_string(), detail: detail.map(str::to_string), error: true });
}

pub fn nudge(app: &AppHandle, index: u32, captures: u32) {
    show(app, NoticePayload::Nudge { index, captures });
}

pub fn show(app: &AppHandle, payload: NoticePayload) {
    let app = app.clone();
    // Window creation and placement are dispatched to the main thread by Tauri; keep callers non-blocking.
    std::thread::spawn(move || show_now(&app, payload));
}

/// Blocks until shown; false when it was dropped (a capture is running, no window), so the caller retries.
/// Never on the main thread.
pub fn show_updated(app: &AppHandle, version: &str, has_notes: bool, set_cleared: bool) -> bool {
    show_now(app, NoticePayload::Updated { version: version.to_string(), has_notes, set_cleared })
}

fn show_now(app: &AppHandle, payload: NoticePayload) -> bool {
    let _serial = NOTICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let skip = |payload: &NoticePayload| {
        // A capture is running: the notice would cover the overlay (or, shown just before, end up in the frame).
        let active = app.state::<AppState>().overlay.lock().unwrap().active;
        if active {
            log::info!("notice skipped during a capture: {payload:?}");
            if matches!(payload, NoticePayload::Nudge { .. }) {
                crate::actions::nudge_settled();
            }
        }
        active
    };
    if skip(&payload) {
        return false;
    }
    let Some(win) = window(app) else {
        if matches!(payload, NoticePayload::Nudge { .. }) {
            crate::actions::nudge_settled();
        }
        return false;
    };
    let is_toast = matches!(payload, NoticePayload::Toast { .. });
    let (w, h) = match payload {
        NoticePayload::Toast { .. } => TOAST_SIZE,
        NoticePayload::Nudge { .. } => CARD_SIZE,
        NoticePayload::Updated { .. } => UPDATED_SIZE,
    };
    let _ = win.set_size(LogicalSize::new(w, h));
    position_bottom_center(app, &win, (w, h), is_toast);
    let _ = win.set_ignore_cursor_events(is_toast);
    if skip(&payload) {
        return false;
    }
    // An error with an explanation needs time to be read.
    let toast_ms = if matches!(payload, NoticePayload::Toast { error: true, detail: Some(_), .. }) {
        ERROR_TOAST_MS
    } else {
        TOAST_MS
    };
    // Bumped before the notice is published, so the timer of an older toast can no longer hide this one.
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    *PENDING.lock().unwrap() = Some(payload.clone());
    let _ = app.emit_to(LABEL, "notice:show", &payload);
    let _ = win.show();
    if is_toast {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(toast_ms));
            // Under the lock: a newer notice is either fully shown (the generation moved on) or not started.
            let _serial = NOTICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            if GENERATION.load(Ordering::SeqCst) == generation {
                hide(&app);
            }
        });
    }
    true
}

/// The page calls this once its listeners are registered: the first notice after the window was created is
/// emitted before the page listens. Not taken: under React StrictMode (dev) the first mount's answer is
/// dropped; `hide` clears it.
pub fn pending() -> Option<NoticePayload> {
    PENDING.lock().unwrap().clone()
}

pub fn hide(app: &AppHandle) {
    PENDING.lock().unwrap().take();
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = app.emit_to(LABEL, "notice:hide", ());
        let _ = win.hide();
    }
}

/// A capture must not include the notice and must not be covered by it. True when a visible notice was
/// hidden, so the caller can wait for the screen to update before capturing.
pub fn hide_for_capture(app: &AppHandle) -> bool {
    let visible = app.get_webview_window(LABEL).is_some_and(|w| w.is_visible().unwrap_or(false));
    if visible {
        hide(app);
    }
    visible
}

/// Destroys the hidden notice window to free its webview; the next notice creates it again. Called from the
/// idle release in overlay.rs, never on the main thread.
pub fn release_if_hidden(app: &AppHandle) -> bool {
    let _serial = NOTICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match app.get_webview_window(LABEL) {
        Some(win) if !win.is_visible().unwrap_or(true) => win.destroy().is_ok(),
        _ => false,
    }
}

fn window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(win) = app.get_webview_window(LABEL) {
        return Some(win);
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("notice.html".into()))
        .title("WellSnap")
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .inner_size(TOAST_SIZE.0, TOAST_SIZE.1)
        .build()
        .map_err(|err| log::error!("could not create notice window: {err}"))
        .ok()
        .inspect(|_win| {
            #[cfg(target_os = "macos")]
            crate::macos::configure_floating(_win);
        })
}

/// Bottom center of the monitor of the last capture (or the primary monitor); cards sit near the tray corner.
fn position_bottom_center(app: &AppHandle, win: &tauri::WebviewWindow, size: (f64, f64), centered: bool) {
    let state = app.state::<AppState>();
    let monitor = {
        let ov = state.overlay.lock().unwrap();
        ov.last_monitor.clone().or_else(|| ov.monitors.first().cloned())
    }
    .or_else(|| crate::capture::monitors(app).into_iter().next());
    let Some(m) = monitor else { return };
    // macOS: global points, independent of the scale of the screen the window is on now. Elsewhere: physical
    // pixels of the target monitor.
    #[cfg(target_os = "macos")]
    {
        let [mx, my, mw, mh] = crate::capture::monitor_points(&m);
        let (x, y) = bottom_center(mx, my, mw, mh, size, 1.0, centered);
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = size;
        let Ok(outer) = win.outer_size() else { return };
        let (x, y) = bottom_center(
            f64::from(m.x),
            f64::from(m.y),
            f64::from(m.width),
            f64::from(m.height),
            (f64::from(outer.width), f64::from(outer.height)),
            m.scale,
            centered,
        );
        let _ = win.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
    }
}

/// Top-left corner for a window of `size` on the monitor rect; margins are in points, `scale` converts them
/// into the rect's units.
fn bottom_center(mx: f64, my: f64, mw: f64, mh: f64, size: (f64, f64), scale: f64, centered: bool) -> (f64, f64) {
    let (w, h) = size;
    let x = if centered { mx + ((mw - w) / 2.0).trunc() } else { mx + mw - w - (24.0 * scale).trunc() };
    let y = my + mh - h - (72.0 * scale).trunc();
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_center_in_physical_pixels() {
        // 1920x1080 at 150 %, a 630x96 physical toast.
        assert_eq!(bottom_center(0.0, 0.0, 1920.0, 1080.0, (630.0, 96.0), 1.5, true), (645.0, 876.0));
        assert_eq!(bottom_center(1920.0, 0.0, 1920.0, 1080.0, (600.0, 354.0), 1.5, false), (3204.0, 618.0));
    }

    #[test]
    fn updated_payload_matches_the_frontend_type() {
        let payload = NoticePayload::Updated { version: "0.2.0".into(), has_notes: true, set_cleared: false };
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            serde_json::json!({"kind": "updated", "version": "0.2.0", "hasNotes": true, "setCleared": false})
        );
    }

    #[test]
    fn bottom_center_on_a_1x_external_monitor_in_points() {
        // MacBook 2x primary, 1x external to the right at x = 1512 pt.
        let m = crate::capture::MonitorInfo {
            id: "1".into(),
            name: "ext".into(),
            x: 1512,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        };
        let [mx, my, mw, mh] = crate::capture::monitor_points(&m);
        assert_eq!(bottom_center(mx, my, mw, mh, TOAST_SIZE, 1.0, true), (1512.0 + 750.0, 1080.0 - 64.0 - 72.0));
    }
}
