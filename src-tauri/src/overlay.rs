//! Capture overlays: one borderless, always-on-top window per monitor, created hidden and reused.
//! Flow: hotkey -> capture every monitor -> each overlay receives its frozen frame -> `overlay_ready` -> show.
//! Frame hand-off: WebView2 shared memory on Windows (webview_win.rs), the `wsnap` protocol elsewhere.
//! Overlays left unused for `IDLE_RELEASE_AFTER` are destroyed to free their webviews and rebuilt on the next
//! capture.

use crate::capture::{self, Frame, MonitorInfo};
use crate::state::AppState;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const READY_TIMEOUT: Duration = Duration::from_millis(2500);
/// A freshly created overlay page needs a moment to load before it can receive a frame.
const PAGE_LOAD_TIMEOUT: Duration = Duration::from_millis(3000);
/// Hidden overlays keep their webviews alive (about 230-290 MB with one 4K monitor, measured), so after this
/// long without a capture they are destroyed; the next capture creates them again, a cold start.
const IDLE_RELEASE_AFTER: Duration = Duration::from_secs(10 * 60);
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// How long the release waits for destroyed windows to disappear before giving the overlays back.
const DESTROY_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
static SCREEN_SETTINGS_OPENED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct OverlayState {
    /// Monitors the current overlay windows were built for.
    pub monitors: Vec<MonitorInfo>,
    /// Overlay pages that finished loading and listen for frames.
    pub loaded: HashSet<String>,
    pub capture_id: u64,
    pub frames: HashMap<String, Arc<Frame>>,
    /// True from the hotkey until the overlays are hidden again (also while idle overlays are destroyed).
    pub active: bool,
    pub shown: HashSet<String>,
    pub started: Option<Instant>,
    /// Monitor where the user last finished a snapshot; the strip and notices appear there.
    pub last_monitor: Option<MonitorInfo>,
    /// A finish for the current capture is running.
    pub finishing: bool,
    /// The overlay that takes keyboard focus in this capture: the one under the cursor.
    pub focus: Option<String>,
    /// When a capture last started or ended, or the overlays were created.
    pub last_used: Option<Instant>,
    /// Idle overlays are being destroyed; a capture requested meanwhile starts right after.
    pub releasing: bool,
    pub pending_capture: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FrameTransport {
    /// Pixels arrive in a `sharedbufferreceived` event (WebView2).
    Shared,
    /// Pixels are fetched from `wsnap://.../frame_path`.
    Protocol,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePayload {
    pub capture_id: u64,
    pub label: String,
    pub monitor: MonitorInfo,
    pub transport: FrameTransport,
    /// Path for the custom protocol (`wsnap`), see protocol.rs. Also valid as a fallback with Shared.
    pub frame_path: String,
    pub width: u32,
    pub height: u32,
    /// Letter the next snapshot gets if it joins the set; None when there is no set yet.
    pub set_letter: Option<char>,
    pub set_count: usize,
    pub set_full: bool,
    pub capture_ms: f64,
}

pub fn label_for(m: &MonitorInfo) -> String {
    format!("overlay-{}", m.id)
}

pub fn is_overlay_label(label: &str) -> bool {
    label.starts_with("overlay-")
}

/// Creates or rebuilds overlay windows so there is exactly one per monitor with matching geometry.
pub fn ensure_windows(app: &AppHandle, monitors: &[MonitorInfo]) {
    let state = app.state::<AppState>();
    let unchanged = state.overlay.lock().unwrap().monitors == monitors;
    if unchanged && monitors.iter().all(|m| app.get_webview_window(&label_for(m)).is_some()) {
        return;
    }
    // Recorded first: a window destroyed below reports Destroyed later, and must not look like a live overlay.
    state.overlay.lock().unwrap().monitors = monitors.to_vec();
    for (label, win) in app.webview_windows() {
        if is_overlay_label(&label) && !monitors.iter().any(|m| label_for(m) == label) {
            state.overlay.lock().unwrap().loaded.remove(&label);
            let _ = win.destroy();
        }
    }
    for m in monitors {
        let label = label_for(m);
        let win = match app.get_webview_window(&label) {
            Some(win) => win,
            None => {
                // A new page must report loading again, whatever happened to the previous window.
                {
                    let mut ov = state.overlay.lock().unwrap();
                    ov.loaded.remove(&label);
                    ov.last_used = Some(Instant::now());
                }
                match WebviewWindowBuilder::new(app, &label, WebviewUrl::App("overlay.html".into()))
                    .title("WellSnap")
                    .decorations(false)
                    .resizable(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .shadow(false)
                    .visible(false)
                    .focused(false)
                    .build()
                {
                    Ok(win) => {
                        #[cfg(target_os = "macos")]
                        crate::macos::configure_overlay(&win);
                        win
                    }
                    Err(err) => {
                        log::error!("could not create overlay {label}: {err}");
                        continue;
                    }
                }
            }
        };
        place(&win, m);
    }
}

fn place(win: &tauri::WebviewWindow, m: &MonitorInfo) {
    // macOS: tao divides physical values by the scale of the screen the window is on now, which is wrong for
    // a window moving to a screen with another scale; global points do not depend on it.
    #[cfg(target_os = "macos")]
    {
        let [x, y, width, height] = capture::monitor_points(m);
        let _ = win.set_position(tauri::LogicalPosition::new(x, y));
        let _ = win.set_size(tauri::LogicalSize::new(width, height));
    }
    // Position first: moving onto a monitor with another DPI rescales the window, so size goes second.
    #[cfg(not(target_os = "macos"))]
    {
        let _ = win.set_position(tauri::PhysicalPosition::new(m.x, m.y));
        let _ = win.set_size(tauri::PhysicalSize::new(m.width, m.height));
    }
}

pub fn on_page_loaded(app: &AppHandle, label: &str) {
    app.state::<AppState>().overlay.lock().unwrap().loaded.insert(label.to_string());
}

/// Entry point for the hotkey, tray and "new snapshot" buttons.
pub fn begin_capture(app: &AppHandle) {
    let state = app.state::<AppState>();
    if !state.overlay.lock().unwrap().request_capture() {
        return;
    }
    crate::telemetry::count("captures_started");
    let notice_hidden = crate::notice::hide_for_capture(app);
    let app = app.clone();
    std::thread::spawn(move || run_capture(&app, notice_hidden));
}

fn run_capture(app: &AppHandle, notice_hidden: bool) {
    let state = app.state::<AppState>();
    if !capture::has_permission() {
        crate::telemetry::count("capture_permission_denied");
        capture::request_permission();
        state.overlay.lock().unwrap().active = false;
        crate::windows_ui::open_onboarding(app);
        return;
    }
    if notice_hidden {
        wait_for_screen_update();
    }
    let monitors = capture::monitors(app);
    crate::telemetry::monitors_seen(&monitors);
    let results = capture::capture_all(&monitors);
    let cursor = cursor_point(app);
    ensure_windows(app, &monitors);

    let (set_letter, set_count, set_full) = {
        let session = state.session.lock().unwrap();
        match session.as_ref() {
            Some(s) if !s.is_empty() => (s.next_letter(), s.snapshots.len(), s.next_letter().is_none()),
            _ => (None, 0, false),
        }
    };

    let transport = if cfg!(windows) { FrameTransport::Shared } else { FrameTransport::Protocol };
    // macOS preflight can say yes after the grant was revoked; only the capture itself tells.
    let denied = results.iter().any(|r| matches!(r, Err(capture::CaptureError::PermissionDenied)));
    let mut prepared = Vec::new();
    let capture_id = {
        let mut ov = state.overlay.lock().unwrap();
        ov.capture_id += 1;
        ov.frames.clear();
        ov.shown.clear();
        ov.finishing = false;
        for (m, result) in monitors.iter().zip(results) {
            match result {
                Ok(frame) => {
                    let label = label_for(m);
                    prepared.push(PreparePayload {
                        capture_id: ov.capture_id,
                        label: label.clone(),
                        monitor: m.clone(),
                        transport,
                        frame_path: format!("frame/{}/{}", ov.capture_id, label),
                        width: frame.width,
                        height: frame.height,
                        set_letter,
                        set_count,
                        set_full,
                        capture_ms: frame.capture_ms,
                    });
                    ov.frames.insert(label, Arc::new(frame));
                }
                Err(err) => log::error!("capture of monitor {} failed: {err}", m.name),
            }
        }
        let candidates: Vec<&MonitorInfo> =
            monitors.iter().filter(|m| prepared.iter().any(|p| p.label == label_for(m))).collect();
        ov.focus = focus_target(&candidates, cursor, monitor_rect);
        ov.capture_id
    };

    if prepared.is_empty() {
        hide_all(app);
        let lang = state.lang();
        if denied {
            crate::telemetry::count("capture_permission_denied");
            let hint = crate::i18n::tr(lang, "toast.screenPermissionHint");
            crate::notice::toast_error(app, crate::i18n::tr(lang, "toast.screenPermissionOff"), Some(hint));
            // Once per run: repeated hotkey presses must not keep pulling System Settings to the front.
            #[cfg(target_os = "macos")]
            if !SCREEN_SETTINGS_OPENED.swap(true, std::sync::atomic::Ordering::Relaxed)
                && let Err(err) = crate::macos::open_privacy_settings("screen")
            {
                log::error!("could not open the Screen Recording settings: {err}");
            }
        } else {
            crate::telemetry::count("capture_failed");
            crate::notice::toast_error(app, crate::i18n::tr(lang, "toast.captureFailed"), None);
        }
        return;
    }
    wait_for_pages(app, &prepared);
    for payload in prepared {
        deliver(app, payload);
    }

    // Watchdog: an overlay that never reports ready must not leave the app stuck in "active".
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(READY_TIMEOUT);
        let state = app.state::<AppState>();
        let stuck = {
            let ov = state.overlay.lock().unwrap();
            ov.active && ov.capture_id == capture_id && ov.shown.is_empty()
        };
        if stuck {
            log::error!("no overlay became ready within {READY_TIMEOUT:?}, cancelling capture");
            crate::telemetry::count("overlay_not_ready");
            hide_all(&app);
        }
    });
}

/// A notice was just hidden: let the compositor present that before the screen is copied, or the toast ends
/// up in the frozen frame. Costs nothing when no notice was visible.
fn wait_for_screen_update() {
    #[cfg(windows)]
    if let Err(err) = unsafe { windows::Win32::Graphics::Dwm::DwmFlush() } {
        log::warn!("DwmFlush: {err}");
    }
    #[cfg(not(windows))]
    std::thread::sleep(Duration::from_millis(16));
}

/// The cursor in the coordinate space of `monitor_rect`.
fn cursor_point(app: &AppHandle) -> Option<(f64, f64)> {
    let p = app.cursor_position().ok()?;
    // macOS: tao returns points times the primary monitor's scale.
    #[cfg(target_os = "macos")]
    {
        let scale = app
            .primary_monitor()
            .ok()
            .flatten()
            .map(|m| m.scale_factor())
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0);
        Some((p.x / scale, p.y / scale))
    }
    #[cfg(not(target_os = "macos"))]
    Some((p.x, p.y))
}

/// Monitor rect in the space the cursor and window placement use: global points on macOS, where each
/// monitor's physical rect has its own scale, and physical pixels elsewhere.
fn monitor_rect(m: &MonitorInfo) -> [f64; 4] {
    #[cfg(target_os = "macos")]
    return capture::monitor_points(m);
    #[cfg(not(target_os = "macos"))]
    [f64::from(m.x), f64::from(m.y), f64::from(m.width), f64::from(m.height)]
}

/// Exactly one overlay takes focus: the first monitor containing the cursor, else the first monitor (no
/// cursor position, a gap between displays), so keyboard shortcuts always work.
fn focus_target(
    monitors: &[&MonitorInfo],
    cursor: Option<(f64, f64)>,
    rect: impl Fn(&MonitorInfo) -> [f64; 4],
) -> Option<String> {
    let under_cursor = cursor.and_then(|(cx, cy)| {
        monitors.iter().find(|m| {
            let [x, y, w, h] = rect(m);
            cx >= x && cx < x + w && cy >= y && cy < y + h
        })
    });
    under_cursor.or(monitors.first()).map(|m| label_for(m))
}

fn wait_for_pages(app: &AppHandle, prepared: &[PreparePayload]) {
    let state = app.state::<AppState>();
    let deadline = Instant::now() + PAGE_LOAD_TIMEOUT;
    while Instant::now() < deadline {
        let all_loaded = {
            let ov = state.overlay.lock().unwrap();
            prepared.iter().all(|p| ov.loaded.contains(&p.label))
        };
        if all_loaded {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    log::warn!("some overlay pages did not report loading in time");
}

fn deliver(app: &AppHandle, payload: PreparePayload) {
    #[cfg(windows)]
    if payload.transport == FrameTransport::Shared {
        let frame = app.state::<AppState>().overlay.lock().unwrap().frames.get(&payload.label).cloned();
        if let (Some(win), Some(frame)) = (app.get_webview_window(&payload.label), frame) {
            let json = serde_json::to_string(&payload).unwrap_or_default();
            let fallback_app = app.clone();
            let mut fallback = payload.clone();
            crate::webview_win::post_frame(&win, frame, json, move |err| {
                log::warn!("shared buffer hand-off failed ({err}), falling back to the protocol");
                fallback.transport = FrameTransport::Protocol;
                let _ = fallback_app.emit_to(fallback.label.as_str(), "overlay:prepare", &fallback);
            });
            return;
        }
    }
    if let Err(err) = app.emit_to(payload.label.as_str(), "overlay:prepare", &payload) {
        log::error!("could not notify {}: {err}", payload.label);
    }
}

/// Called by an overlay after its frozen frame is drawn, so the window never appears empty or stale.
pub fn on_ready(app: &AppHandle, label: &str, capture_id: u64) {
    let state = app.state::<AppState>();
    let (elapsed, monitor, focus) = {
        let mut ov = state.overlay.lock().unwrap();
        if !ov.active || ov.capture_id != capture_id {
            return;
        }
        ov.shown.insert(label.to_string());
        let monitor = ov.monitors.iter().find(|m| label_for(m) == label).cloned();
        (ov.started.map(|t| t.elapsed()), monitor, ov.focus.as_deref() == Some(label))
    };
    let Some(win) = app.get_webview_window(label) else { return };
    if let Some(m) = monitor {
        place(&win, &m);
    }
    let _ = win.show();
    // On macOS tao's always-on-top is the floating level, below the menu bar and the Dock.
    #[cfg(target_os = "macos")]
    crate::macos::overlay_shown(&win);
    #[cfg(not(target_os = "macos"))]
    let _ = win.set_always_on_top(true);
    if focus {
        let _ = win.set_focus();
    }
    if let Some(elapsed) = elapsed {
        log::info!("overlay {label} visible {:.0} ms after hotkey", elapsed.as_secs_f64() * 1000.0);
        // Once per capture: the overlay that takes focus is the one under the cursor.
        if focus {
            crate::telemetry::metric(label, "hotkeyToVisibleMs", elapsed.as_secs_f64() * 1000.0);
        }
    }
}

pub fn hide_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    for (label, win) in app.webview_windows() {
        if is_overlay_label(&label) {
            // Reset frees the frame. Both calls are queued and the script runs asynchronously, so this order
            // does not guarantee a repaint before the hide; whether a hidden WKWebView flashes the previous
            // screenshot on the next show is still to be checked on a Mac.
            let _ = app.emit_to(label.as_str(), "overlay:reset", ());
            let _ = win.hide();
        }
    }
    #[cfg(windows)]
    crate::webview_win::release_all(app);
    // Give keyboard focus back to the app the user was in (the chat to paste into).
    #[cfg(target_os = "macos")]
    crate::macos::overlays_hidden(app);
    let mut ov = state.overlay.lock().unwrap();
    ov.active = false;
    ov.finishing = false;
    ov.frames.clear();
    ov.shown.clear();
    ov.last_used = Some(Instant::now());
}

/// Overlay windows are closed by the OS too (Alt+F4 in a comment field, a crashed webview). A close request
/// counts as cancel and keeps the window; a destroyed window must not leave the capture stuck in `active` or
/// its page counted as loaded.
pub fn on_window_event(app: &AppHandle, label: &str, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let state = app.state::<AppState>();
            let cancel = {
                let ov = state.overlay.lock().unwrap();
                ov.active && !ov.finishing
            };
            if cancel {
                hide_all(app);
            }
        }
        tauri::WindowEvent::Destroyed => {
            let state = app.state::<AppState>();
            let in_capture = {
                let mut ov = state.overlay.lock().unwrap();
                ov.loaded.remove(label);
                // Not an overlay of this capture: rebuilt for other monitors, or released while idle.
                ov.active && !ov.releasing && ov.monitors.iter().any(|m| label_for(m) == label)
            };
            if in_capture {
                log::warn!("overlay {label} was destroyed during a capture");
                hide_all(app);
            }
        }
        _ => {}
    }
}

/// Every `IDLE_CHECK_INTERVAL`: destroys overlays unused for `IDLE_RELEASE_AFTER`, and the notice and strip
/// windows if they are hidden. Settings and onboarding are the user's windows and stay.
pub fn start_idle_release(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(IDLE_CHECK_INTERVAL);
            release_if_idle(&app);
        }
    });
}

fn release_if_idle(app: &AppHandle) {
    let state = app.state::<AppState>();
    let has_overlays = app.webview_windows().keys().any(|label| is_overlay_label(label));
    {
        let mut ov = state.overlay.lock().unwrap();
        if ov.active || !is_idle(ov.last_used, Instant::now()) {
            return;
        }
        if has_overlays {
            // Claimed like a capture, so none can start while the windows go away.
            ov.active = true;
            ov.releasing = true;
            ov.monitors.clear();
            ov.loaded.clear();
        }
    }
    let overlays = has_overlays;
    let mut pending = false;
    if overlays {
        let windows: Vec<_> = app.webview_windows().into_iter().filter(|(label, _)| is_overlay_label(label)).collect();
        for (label, win) in &windows {
            if let Err(err) = win.destroy() {
                log::warn!("could not destroy {label}: {err}");
            }
        }
        let deadline = Instant::now() + DESTROY_TIMEOUT;
        while windows.iter().any(|(label, _)| app.get_webview_window(label).is_some()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut ov = state.overlay.lock().unwrap();
        ov.loaded.clear();
        ov.active = false;
        ov.releasing = false;
        pending = std::mem::take(&mut ov.pending_capture);
    }
    let notice = crate::notice::release_if_hidden(app);
    let strip = crate::windows_ui::release_strip_if_hidden(app);
    if overlays || notice || strip {
        log::info!("released idle overlays (overlays: {overlays}, notice: {notice}, strip: {strip})");
    }
    if pending {
        begin_capture(app);
    }
}

fn is_idle(last_used: Option<Instant>, now: Instant) -> bool {
    last_used.is_some_and(|t| now.saturating_duration_since(t) >= IDLE_RELEASE_AFTER)
}

impl OverlayState {
    /// True when a capture starts now. While the idle release or an update holds the overlays, the request is
    /// kept in `pending_capture` for when they are given back.
    fn request_capture(&mut self) -> bool {
        if self.active {
            if self.releasing {
                self.pending_capture = true;
            }
            return false;
        }
        self.active = true;
        self.started = Some(Instant::now());
        self.last_used = self.started;
        true
    }

    /// Claimed like the idle release: no capture starts, and a hotkey press is kept in `pending_capture`.
    fn claim_for_update(&mut self) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.releasing = true;
        true
    }

    /// Returns whether a capture was requested meanwhile.
    fn end_update_claim(&mut self) -> bool {
        self.active = false;
        self.releasing = false;
        std::mem::take(&mut self.pending_capture)
    }
}

/// The updater is about to replace the app: no capture may start until it exits or the install fails.
pub fn try_claim_for_update(app: &AppHandle) -> bool {
    app.state::<AppState>().overlay.lock().unwrap().claim_for_update()
}

/// The install failed: give the overlays back and run a capture requested meanwhile.
pub fn release_update_claim(app: &AppHandle) {
    let pending = app.state::<AppState>().overlay.lock().unwrap().end_update_claim();
    if pending {
        begin_capture(app);
    }
}

pub fn frame(app: &AppHandle, capture_id: u64, label: &str) -> Option<Arc<Frame>> {
    let state = app.state::<AppState>();
    let ov = state.overlay.lock().unwrap();
    (ov.capture_id == capture_id).then(|| ov.frames.get(label).cloned()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: &str, x: i32, y: i32, width: u32, height: u32, scale: f64) -> MonitorInfo {
        MonitorInfo { id: id.into(), name: id.into(), x, y, width, height, scale }
    }

    fn physical(m: &MonitorInfo) -> [f64; 4] {
        [f64::from(m.x), f64::from(m.y), f64::from(m.width), f64::from(m.height)]
    }

    #[test]
    fn focus_goes_to_exactly_one_overlay_on_mixed_scale_macs() {
        // 2x MacBook primary and a 1x external to its right at 1512 pt; the cursor is in points.
        let primary = monitor("0", 0, 0, 3024, 1964, 2.0);
        let external = monitor("1", 1512, 0, 1920, 1080, 1.0);
        let list = [&primary, &external];
        let target = |x: f64, y: f64| focus_target(&list, Some((x, y)), capture::monitor_points);
        assert_eq!(target(1000.0, 500.0).as_deref(), Some("overlay-0"));
        assert_eq!(target(1600.0, 500.0).as_deref(), Some("overlay-1"));
        assert_eq!(target(1511.9, 981.0).as_deref(), Some("overlay-0"));
        // Below the shorter screen's bottom edge, in no monitor: the first one still takes focus.
        assert_eq!(target(3000.0, 1200.0).as_deref(), Some("overlay-0"));
    }

    #[test]
    fn focus_in_physical_pixels_and_fallbacks() {
        let left = monitor("0", -1920, 0, 1920, 1080, 1.0);
        let main = monitor("1", 0, 0, 3840, 2160, 1.5);
        let list = [&left, &main];
        assert_eq!(focus_target(&list, Some((-10.0, 10.0)), physical).as_deref(), Some("overlay-0"));
        assert_eq!(focus_target(&list, Some((0.0, 0.0)), physical).as_deref(), Some("overlay-1"));
        assert_eq!(focus_target(&list, None, physical).as_deref(), Some("overlay-0"));
        assert_eq!(focus_target(&[], Some((0.0, 0.0)), physical), None);
    }

    #[test]
    fn update_claim_keeps_a_hotkey_press_for_later() {
        let mut ov = OverlayState { active: true, ..Default::default() };
        assert!(!ov.claim_for_update(), "a capture is running");
        ov.active = false;
        assert!(ov.claim_for_update());
        assert!(ov.active && ov.releasing);
        assert!(!ov.claim_for_update());
        assert!(!ov.request_capture(), "a hotkey press while claimed waits");
        assert!(ov.end_update_claim());
        assert!(!ov.active && !ov.releasing && !ov.pending_capture);
        assert!(ov.claim_for_update());
        assert!(!ov.end_update_claim());
    }

    #[test]
    fn a_capture_request_starts_one_capture() {
        let mut ov = OverlayState::default();
        assert!(ov.request_capture());
        assert!(ov.active && ov.started.is_some() && ov.last_used == ov.started);
        assert!(!ov.request_capture(), "one capture at a time");
        assert!(!ov.pending_capture, "a second press during a capture is dropped");
    }

    #[test]
    fn idle_after_ten_minutes_only() {
        let now = Instant::now();
        assert!(!is_idle(None, now), "never used: nothing to release");
        assert!(!is_idle(Some(now), now));
        let earlier = now.checked_sub(IDLE_RELEASE_AFTER).unwrap();
        assert!(is_idle(Some(earlier), now));
        assert!(!is_idle(Some(earlier + Duration::from_secs(1)), now));
    }
}
