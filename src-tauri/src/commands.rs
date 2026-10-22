//! Commands callable from the webviews (`invoke`). Keep in sync with src/shared/ipc.ts.
//! Synchronous commands run on the main thread; anything that touches the clipboard, the disk, the registry
//! or broadcasts window messages is async so it runs on the async runtime instead.

use crate::actions::{self, FinishMeta};
use crate::conflicts::ConflictReport;
use crate::donations::PromptOutcome;
use crate::i18n::Lang;
use crate::printscreen::{self, PrintScreenStatus};
use crate::session::Snapshot;
use crate::settings::{SetCopyMode, Settings};
use crate::sheet::{self, SheetPutMeta, SheetRequest};
use crate::state::{AppState, HotkeyStatus};
use crate::telemetry::{self, CrashAnswer, JsErrorReport, SentItem, TelemetryStatus};
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

/// Hosts the app may open in the browser (support page, donation links, project page).
const EXTERNAL_HOSTS: &[&str] = &["github.com", "pay.cloudtips.ru", "cloudtips.ru", "wellsnap.app", "tonviewer.com"];

#[tauri::command]
pub fn overlay_page_loaded(app: AppHandle, window: WebviewWindow) {
    crate::overlay::on_page_loaded(&app, window.label());
}

#[tauri::command]
pub fn overlay_ready(app: AppHandle, window: WebviewWindow, capture_id: u64) {
    crate::overlay::on_ready(&app, window.label(), capture_id);
}

/// Ignored while a finish runs: Esc on another monitor must not end the capture under it.
#[tauri::command]
pub fn overlay_cancel(app: AppHandle) {
    let finishing = app.state::<AppState>().overlay.lock().unwrap().finishing;
    if !finishing {
        telemetry::count("captures_cancelled");
        crate::overlay::hide_all(&app);
    }
}

/// Body: the encoded image (raw bytes). Header `x-meta`: URI-encoded JSON of `FinishMeta`.
#[tauri::command]
pub async fn overlay_finish(app: AppHandle, window: WebviewWindow, request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected a raw image body".into());
    };
    let meta_header = request
        .headers()
        .get("x-meta")
        .and_then(|v| v.to_str().ok())
        .ok_or("missing x-meta header")?;
    let meta_json = urlencoding::decode(meta_header).map_err(|e| e.to_string())?;
    let meta: FinishMeta = serde_json::from_str(&meta_json).map_err(|e| e.to_string())?;
    actions::finish(&app, window.label(), meta, bytes.clone())
}

#[tauri::command]
pub fn log_metric(window: WebviewWindow, name: String, value: f64) {
    log::info!("[metric] {} {name} = {value:.1}", window.label());
    telemetry::metric(window.label(), &name, value);
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

/// Fields only the core writes: donation counters, the Print Screen take-over flag and the fingerprint of the
/// last hotkey warning.
const CORE_OWNED: &[&str] = &["donations", "printScreenTakenOver", "hotkeyWarning"];

/// Shallow merge of `patch` into the settings. Core-owned fields are not writable from here.
#[tauri::command]
pub fn update_settings(app: AppHandle, patch: serde_json::Value) -> Result<Settings, String> {
    let state = app.state::<AppState>();
    let before = state.settings();
    let mut value = serde_json::to_value(&before).map_err(|e| e.to_string())?;
    let (Some(target), Some(patch)) = (value.as_object_mut(), patch.as_object()) else {
        return Err("patch must be an object".into());
    };
    for (key, v) in patch {
        if !CORE_OWNED.contains(&key.as_str()) {
            target.insert(key.clone(), v.clone());
        }
    }
    let merged: Settings = serde_json::from_value(value).map_err(|e| e.to_string())?;
    // The login item first: if the system refuses, nothing is saved and the switch flips back.
    if merged.autostart != before.autostart {
        crate::autostart::apply(&app, merged.autostart)?;
    }
    // Core-owned fields come from the locked value, not from `before`: a finish on another thread may have
    // counted a capture in between.
    let after = state.update_settings(|s| *s = keep_core_owned(merged, s));
    apply_side_effects(&app, &before, &after);
    Ok(after)
}

fn keep_core_owned(mut merged: Settings, current: &mut Settings) -> Settings {
    merged.donations = std::mem::take(&mut current.donations);
    merged.print_screen_taken_over = current.print_screen_taken_over;
    // The warning was about the old capture key; a new key gets its own.
    merged.hotkey_warning =
        if merged.capture_hotkey == current.capture_hotkey { current.hotkey_warning.take() } else { None };
    merged
}

fn apply_side_effects(app: &AppHandle, before: &Settings, after: &Settings) {
    if before.capture_hotkey != after.capture_hotkey || before.smart_paste_hotkey != after.smart_paste_hotkey {
        crate::hotkeys::apply(app);
    }
    if before.theme != after.theme {
        crate::windows_ui::apply_theme(app, after.theme);
    }
    if before.hide_strip_from_capture != after.hide_strip_from_capture {
        crate::windows_ui::sync_strip(app);
    }
    // The headings inside a sheet are in the UI language.
    if before.set_copy_mode != after.set_copy_mode || before.language != after.language {
        sheet::request_prewarm(app);
    }
    telemetry::settings_changed(app, before, after);
    crate::tray::refresh(app);
    let _ = app.emit("settings:changed", after);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    platform: &'static str,
    lang: Lang,
    hotkeys: HotkeyStatus,
    print_screen: PrintScreenStatus,
    screen_permission: bool,
    input_permission: bool,
    save_dir: String,
    /// This build can send crash reports and statistics (see telemetry::available).
    telemetry: bool,
}

#[tauri::command]
pub async fn get_app_info(app: AppHandle) -> AppInfo {
    let state = app.state::<AppState>();
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "other" },
        lang: state.lang(),
        hotkeys: state.hotkeys.lock().unwrap().clone(),
        print_screen: printscreen::status(),
        screen_permission: crate::capture::has_permission(),
        input_permission: crate::paste::can_send_input(),
        save_dir: actions::save_dir(&app).to_string_lossy().into_owned(),
        telemetry: telemetry::available(),
    }
}

#[tauri::command]
pub fn hotkeys_suspend(app: AppHandle) {
    crate::hotkeys::suspend(&app);
}

#[tauri::command]
pub fn hotkeys_resume(app: AppHandle) {
    crate::hotkeys::resume(&app);
}

#[tauri::command]
pub fn validate_hotkey(accelerator: String) -> Result<(), String> {
    crate::hotkeys::validate(&accelerator)
}

/// Async: the scan lists the running processes.
#[tauri::command]
pub async fn hotkey_conflicts(app: AppHandle) -> ConflictReport {
    crate::conflicts::report(&app)
}

/// Registers the hotkeys again (the other program may have let the key go) and returns the outcome.
#[tauri::command]
pub async fn hotkeys_recheck(app: AppHandle) -> HotkeyStatus {
    crate::hotkeys::apply(&app);
    crate::hotkeys::settled_status(&app)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetInfo {
    session_id: Option<String>,
    snapshots: Vec<Snapshot>,
    next_letter: Option<char>,
    text: String,
    smart_paste_hotkey: String,
    smart_paste_registered: bool,
    copy_mode: SetCopyMode,
}

#[tauri::command]
pub fn get_set(app: AppHandle) -> SetInfo {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let settings = state.settings();
    let registered = state.hotkeys.lock().unwrap().smart_paste_registered;
    let guard = state.session.lock().unwrap();
    match guard.as_ref() {
        Some(s) => SetInfo {
            session_id: Some(s.id.clone()),
            snapshots: s.snapshots.clone(),
            next_letter: s.next_letter(),
            text: s.text_block(lang),
            smart_paste_hotkey: settings.smart_paste_hotkey,
            smart_paste_registered: registered,
            copy_mode: settings.set_copy_mode,
        },
        None => SetInfo {
            session_id: None,
            snapshots: Vec::new(),
            next_letter: Some('A'),
            text: String::new(),
            smart_paste_hotkey: settings.smart_paste_hotkey,
            smart_paste_registered: registered,
            copy_mode: settings.set_copy_mode,
        },
    }
}

#[tauri::command]
pub async fn set_copy_all(app: AppHandle) -> Result<(), String> {
    actions::copy_all(&app).await
}

#[tauri::command]
pub async fn set_save_all(app: AppHandle) -> Result<usize, String> {
    actions::save_all(&app).await
}

/// Body: the composed sheet (raw PNG). Header `x-meta`: URI-encoded JSON of `SheetPutMeta`.
#[tauri::command]
pub async fn sheet_put(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected a raw image body".into());
    };
    let meta_header = request
        .headers()
        .get("x-meta")
        .and_then(|v| v.to_str().ok())
        .ok_or("missing x-meta header")?;
    let meta_json = urlencoding::decode(meta_header).map_err(|e| e.to_string())?;
    let meta: SheetPutMeta = serde_json::from_str(&meta_json).map_err(|e| e.to_string())?;
    sheet::accept(&app, meta, bytes.clone())
}

#[tauri::command]
pub fn sheet_failed(app: AppHandle, request_id: u64, reason: String) {
    sheet::fail(&app, request_id, reason);
}

/// The strip subscribed to `sheet:request`: returns the request it may have missed while loading.
#[tauri::command]
pub fn sheet_pending(app: AppHandle) -> Option<SheetRequest> {
    sheet::pending(&app)
}

#[tauri::command]
pub fn set_clear(app: AppHandle) {
    actions::clear_set(&app);
}

#[tauri::command]
pub fn set_remove(app: AppHandle, letter: char) {
    actions::remove_from_set(&app, letter);
}

#[tauri::command]
pub fn start_capture(app: AppHandle) {
    crate::overlay::begin_capture(&app);
}

/// Async: building a window inside a synchronous command deadlocks on Windows (WebView2 IPC callback).
#[tauri::command]
pub async fn open_settings(app: AppHandle, tab: Option<String>) {
    crate::windows_ui::open_settings(&app, tab.as_deref());
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = allowed_external(&url).ok_or_else(|| format!("blocked url: {url}"))?;
    // The normalized form, so the string that was checked is the string that gets opened.
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(|e| e.to_string())
}

/// https on one of `EXTERNAL_HOSTS` (or a subdomain), no credentials, no explicit port. A backslash is refused
/// outright: browsers read it as a slash, so "https://evil.com\.github.com" would open evil.com.
fn allowed_external(url: &str) -> Option<tauri::Url> {
    if url.contains('\\') {
        return None;
    }
    let parsed = tauri::Url::parse(url).ok()?;
    let host_ok = parsed
        .host_str()
        .is_some_and(|host| EXTERNAL_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))));
    let ok = parsed.scheme() == "https"
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && host_ok;
    ok.then_some(parsed)
}

#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn print_screen_status() -> PrintScreenStatus {
    printscreen::status()
}

#[tauri::command]
pub async fn print_screen_take_over(app: AppHandle) -> Result<PrintScreenStatus, String> {
    printscreen::take_over()?;
    set_print_screen_taken_over(&app, true);
    crate::hotkeys::apply(&app);
    Ok(printscreen::status())
}

#[tauri::command]
pub async fn print_screen_restore(app: AppHandle) -> Result<PrintScreenStatus, String> {
    printscreen::restore()?;
    set_print_screen_taken_over(&app, false);
    crate::hotkeys::apply(&app);
    Ok(printscreen::status())
}

/// Remembers the choice in the settings file, which survives a reinstall (see lib.rs, startup).
fn set_print_screen_taken_over(app: &AppHandle, taken_over: bool) {
    let settings = app.state::<AppState>().update_settings(|s| s.print_screen_taken_over = taken_over);
    let _ = app.emit("settings:changed", &settings);
}

#[tauri::command]
pub fn donation_outcome(app: AppHandle, outcome: PromptOutcome) {
    actions::nudge_settled();
    let state = app.state::<AppState>();
    let now = chrono::Utc::now().timestamp();
    let settings = state.update_settings(|s| s.donations.apply_outcome(now, outcome));
    crate::tray::refresh(&app);
    let _ = app.emit("settings:changed", &settings);
}

/// Settings "Support" tab: honor-system supporter flag and reminder mode.
#[tauri::command]
pub fn donation_set_status(app: AppHandle, supporter: bool, reminders: bool) {
    let state = app.state::<AppState>();
    let now = chrono::Utc::now().timestamp();
    let settings = state.update_settings(|s| {
        if supporter {
            s.donations.apply_outcome(now, PromptOutcome::AlreadySupported);
        } else if !reminders {
            s.donations.apply_outcome(now, PromptOutcome::Never);
        } else if s.donations.status != crate::donations::DonationStatus::Active {
            s.donations.reactivate(now);
        }
    });
    crate::tray::refresh(&app);
    let _ = app.emit("settings:changed", &settings);
}

#[tauri::command]
pub fn donation_reset_counters(app: AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.update_settings(|s| s.donations.reset_counters());
    let _ = app.emit("settings:changed", &settings);
}

#[tauri::command]
pub fn notice_close(app: AppHandle) {
    crate::notice::hide(&app);
}

/// The notice page registered its listeners: returns the notice on screen, which it may have missed while
/// loading. The page ignores it if a newer event already arrived.
#[tauri::command]
pub fn notice_ready() -> Option<crate::notice::NoticePayload> {
    crate::notice::pending()
}

#[tauri::command]
pub fn onboarding_complete(app: AppHandle) {
    let state = app.state::<AppState>();
    state.update_settings(|s| s.onboarding_done = true);
    if let Some(win) = app.get_webview_window("onboarding") {
        let _ = win.close();
    }
}

#[tauri::command]
pub fn request_screen_permission() -> bool {
    crate::capture::request_permission()
}

#[tauri::command]
pub fn request_input_permission() -> bool {
    crate::paste::request_input_permission()
}

/// Opens a system settings page: "keyboard" (Windows Print Screen toggle), "screen" or "accessibility"
/// (macOS privacy panes).
#[tauri::command]
pub fn open_system_settings(app: AppHandle, kind: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return {
        let _ = &app;
        crate::macos::open_privacy_settings(&kind)
    };
    #[cfg(not(target_os = "macos"))]
    return match kind.as_str() {
        "keyboard" => app.opener().open_url("ms-settings:easeofaccess-keyboard", None::<&str>).map_err(|e| e.to_string()),
        _ => Err(format!("unknown settings page: {kind}")),
    };
}

/// Known update state and the recently installed version, without a network request. Async: the first call
/// on macOS probes whether the app folder is writable.
#[tauri::command]
pub async fn get_update(app: AppHandle) -> crate::updates::UpdateStatus {
    crate::updates::status(&app)
}

#[tauri::command]
pub fn hide_update_notes(app: AppHandle) {
    crate::updates::hide_notes(&app);
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<crate::updates::UpdateInfo>, String> {
    crate::updates::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    crate::updates::install(&app).await
}

#[tauri::command]
pub fn relaunch(app: AppHandle) {
    app.restart();
}

/// Telemetry commands are async: they read and write the outbox and the sent log.
#[tauri::command]
pub async fn report_js_error(app: AppHandle, window: WebviewWindow, report: JsErrorReport) {
    telemetry::report_js_error(&app, window.label(), report);
}

#[tauri::command]
pub async fn telemetry_status(app: AppHandle) -> TelemetryStatus {
    telemetry::status(&app)
}

#[tauri::command]
pub async fn telemetry_history(app: AppHandle) -> Vec<SentItem> {
    telemetry::history(&app)
}

#[tauri::command]
pub async fn crash_prompt_answer(app: AppHandle, answer: CrashAnswer) {
    telemetry::crash_prompt_answer(&app, answer);
}

#[tauri::command]
pub async fn telemetry_reset_ids(app: AppHandle) {
    telemetry::reset_ids(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_urls_on_allowed_hosts_only() {
        for ok in [
            "https://github.com/hashtriton/wellsnap",
            "https://sub.github.com/x",
            "https://pay.cloudtips.ru/p/123?a=1#b",
            "https://github.com:443/x",
        ] {
            assert!(allowed_external(ok).is_some(), "{ok}");
        }
        for blocked in [
            "https://evil.com\\.github.com/",
            "https://evil.example\\.github.com/x",
            "https://evil.com@github.com",
            "https://github.com@evil.com/",
            "https://user@github.com/",
            "https://user:pass@github.com/",
            "https://github.com:8443",
            "http://github.com/",
            "https://evilgithub.com/",
            "https://github.com.evil.com/",
            "javascript:alert(1)",
            "not a url",
        ] {
            assert!(allowed_external(blocked).is_none(), "{blocked}");
        }
    }

    #[test]
    fn opened_url_is_the_normalized_one() {
        assert_eq!(allowed_external("https://GitHub.com/a/../b").unwrap().as_str(), "https://github.com/b");
    }

    #[test]
    fn core_owned_fields_survive_a_stale_merge() {
        let mut current = Settings { print_screen_taken_over: true, ..Default::default() };
        current.donations.captures = 8;
        let stale = Settings { capture_hotkey: "Ctrl+Shift+X".into(), ..Default::default() };
        let merged = keep_core_owned(stale, &mut current);
        assert_eq!(merged.donations.captures, 8);
        assert!(merged.print_screen_taken_over);
        assert_eq!(merged.capture_hotkey, "Ctrl+Shift+X");
    }

    #[test]
    fn hotkey_warning_is_kept_until_the_capture_key_changes() {
        const SHOWN: &str = "PrintScreen|taken|oneDrive";
        let warned = || Settings { hotkey_warning: Some(SHOWN.into()), ..Default::default() };
        // A webview patch never clears or forges it.
        let stale = Settings { theme: crate::settings::Theme::Dark, ..Default::default() };
        assert_eq!(keep_core_owned(stale, &mut warned()).hotkey_warning.as_deref(), Some(SHOWN));
        let forged = Settings { hotkey_warning: Some("x".into()), ..Default::default() };
        assert_eq!(keep_core_owned(forged, &mut warned()).hotkey_warning.as_deref(), Some(SHOWN));
        // A new capture key starts without a warning.
        let changed = Settings { capture_hotkey: "Ctrl+PrintScreen".into(), ..Default::default() };
        assert_eq!(keep_core_owned(changed, &mut warned()).hotkey_warning, None);
    }

    #[test]
    fn set_and_app_info_carry_the_new_fields_for_ipc() {
        let set = SetInfo {
            session_id: None,
            snapshots: vec![],
            next_letter: Some('A'),
            text: String::new(),
            smart_paste_hotkey: String::new(),
            smart_paste_registered: false,
            copy_mode: SetCopyMode::OnePicture,
        };
        assert_eq!(serde_json::to_value(set).unwrap()["copyMode"], "onePicture");
        let info = AppInfo {
            version: String::new(),
            platform: "windows",
            lang: Lang::En,
            hotkeys: HotkeyStatus::default(),
            print_screen: PrintScreenStatus::default(),
            screen_permission: true,
            input_permission: true,
            save_dir: String::new(),
            telemetry: false,
        };
        assert_eq!(serde_json::to_value(info).unwrap()["telemetry"], false);
    }
}
