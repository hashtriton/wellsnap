//! Crash reports (Sentry) and anonymous daily usage totals (PostHog), sent directly by the app. Only a build
//! with the "telemetry" cargo feature and the keys given at build time (WELLSNAP_SENTRY_DSN,
//! WELLSNAP_POSTHOG_KEY, optional WELLSNAP_POSTHOG_HOST) can send anything; forks and local builds stay
//! silent. Nothing goes out before onboarding is done, and the settings `crash_reports` and `usage_stats`
//! turn either part off. Screenshots, clipboard, comments, file names and window titles are never sent.
//!
//! Without the feature every function below is a no-op. With it:
//! - crash.rs: the Sentry client (panics, minidumps, log errors, webview crashes), scrubbing hooks, consent;
//! - outbox.rs: the disk transport every report goes through, and the outbox rules;
//! - upload.rs: the thread that sends the outbox to Sentry and the usage events to PostHog;
//! - usage.rs: daily counters and the events built from them (names in events.json);
//! - js.rs: JavaScript errors from the webviews; scrub.rs: removing user and machine names;
//! - store.rs: telemetry.json (ids, flags, counters) and the "what was sent" log; paths.rs: where files live.
#![cfg_attr(not(feature = "telemetry"), allow(dead_code))] // Report fields are read only with the feature.

#[cfg(feature = "telemetry")]
mod crash;
#[cfg(feature = "telemetry")]
mod js;
#[cfg(feature = "telemetry")]
mod outbox;
#[cfg(feature = "telemetry")]
mod paths;
#[cfg(feature = "telemetry")]
mod scrub;
#[cfg(feature = "telemetry")]
mod store;
#[cfg(feature = "telemetry")]
mod upload;
#[cfg(feature = "telemetry")]
mod usage;

use crate::actions::FinishMeta;
use crate::capture::MonitorInfo;
use crate::session::Mark;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::AppHandle;

/// Held for the whole run (crash handler and reporter process).
pub struct Guard {
    #[cfg(feature = "telemetry")]
    _client: Option<sentry::ClientInitGuard>,
}

/// First thing in `run()`, before Tauri and single-instance: the crash reporter process re-runs the same
/// binary and never returns from here.
#[cfg(feature = "telemetry")]
pub fn early_init() -> Guard {
    Guard { _client: crash::early_init() }
}

#[cfg(not(feature = "telemetry"))]
pub fn early_init() -> Guard {
    Guard {}
}

/// Extra log target: errors from WellSnap's own code become reports, other lines become breadcrumbs.
pub fn log_dispatch() -> tauri_plugin_log::fern::Dispatch {
    let dispatch = tauri_plugin_log::fern::Dispatch::new();
    #[cfg(feature = "telemetry")]
    let dispatch = dispatch.chain(tauri_plugin_log::fern::Output::call(crash::on_log));
    dispatch
}

/// In `setup`, once `AppState` exists: crash prompt, uploader and usage counters. An install that finished
/// onboarding in a version without telemetry never saw the onboarding card: telemetry.json does not record
/// `consentSeen` then, so Settings > Privacy opens once before anything is sent.
pub fn start(app: &AppHandle) {
    #[cfg(feature = "telemetry")]
    crash::start(app);
    #[cfg(not(feature = "telemetry"))]
    let _ = app;
}

/// `RunEvent::Exit`: marks the run as a clean exit.
pub fn shutdown() {
    #[cfg(feature = "telemetry")]
    crash::shutdown();
}

/// This build can send: compiled with the feature and given both keys. The UI shows the crash modes and the
/// statistics switch together on this one flag, so a build with only one key sends nothing at all, and every
/// sending path must check this first.
pub fn available() -> bool {
    configured(cfg!(feature = "telemetry"), option_env!("WELLSNAP_SENTRY_DSN"), option_env!("WELLSNAP_POSTHOG_KEY"))
}

fn configured(feature: bool, sentry_dsn: Option<&str>, posthog_key: Option<&str>) -> bool {
    feature && is_key(sentry_dsn) && is_key(posthog_key)
}

/// A secret missing in a fork's CI arrives as an empty variable.
fn is_key(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.trim().is_empty())
}

/// Consent side effects of a settings change (statistics off deletes the usage id and pending events,
/// crash reports "never" empties the outbox).
pub fn settings_changed(app: &AppHandle, before: &Settings, after: &Settings) {
    #[cfg(feature = "telemetry")]
    crash::settings_changed(app, before, after);
    #[cfg(not(feature = "telemetry"))]
    let _ = (app, before, after);
}

/// The onboarding window closed (finished or dismissed); sending may start. Called on every close: on macOS
/// the window opens again when screen permission is missing, and `onboarding_done` is already true when the
/// "Done" button closes it, so one-time work (the `onboarding_finished` event) is recorded in telemetry.json
/// and done once.
pub fn onboarding_finished(app: &AppHandle) {
    #[cfg(feature = "telemetry")]
    crash::onboarding_finished(app);
    #[cfg(not(feature = "telemetry"))]
    let _ = app;
}

/// macOS killed a webview's WebContent process (the page is blank until it loads again). Registered in
/// `lib.rs`; WKWebView does nothing on its own, so the report and the reload both belong here.
#[cfg(target_os = "macos")]
pub fn web_content_terminated(webview: &tauri::Webview<tauri::Wry>) {
    #[cfg(feature = "telemetry")]
    crash::webview_failed(webview.label(), "web_content_terminated", None, None);
    if reload_allowed(webview.label()) {
        if let Err(err) = webview.reload() {
            log::warn!("could not reload {} after its WebContent process ended: {err}", webview.label());
        }
    } else {
        log::warn!("{} keeps losing its WebContent process; not reloading it again", webview.label());
    }
}

/// At most 3 reloads of one webview within a minute: a page that kills its process on load would loop.
#[cfg(target_os = "macos")]
fn reload_allowed(label: &str) -> bool {
    use std::sync::Mutex;
    static RELOADS: Mutex<Vec<(String, std::time::Instant)>> = Mutex::new(Vec::new());
    let mut reloads = RELOADS.lock().unwrap_or_else(|e| e.into_inner());
    allow_reload(&mut reloads, label, std::time::Instant::now())
}

#[cfg(any(target_os = "macos", test))]
fn allow_reload(reloads: &mut Vec<(String, std::time::Instant)>, label: &str, now: std::time::Instant) -> bool {
    reloads.retain(|(_, at)| now.duration_since(*at) < std::time::Duration::from_secs(60));
    if reloads.iter().filter(|(l, _)| l == label).count() >= 3 {
        return false;
    }
    reloads.push((label.to_string(), now));
    true
}

/// Adds one to a daily usage counter (names as in telemetry/events.json).
pub fn count(name: &'static str) {
    #[cfg(feature = "telemetry")]
    usage::add(name, 1);
    #[cfg(not(feature = "telemetry"))]
    let _ = name;
}

/// A timing or size reported by a webview through `log_metric` (or by the core, page "core").
pub fn metric(page: &str, name: &str, value: f64) {
    #[cfg(feature = "telemetry")]
    usage::metric(page, name, value);
    #[cfg(not(feature = "telemetry"))]
    let _ = (page, name, value);
}

/// A capture ran on these monitors (the daily `monitors` and `max_scale` values).
pub fn monitors_seen(monitors: &[MonitorInfo]) {
    #[cfg(feature = "telemetry")]
    usage::monitors_seen(monitors);
    #[cfg(not(feature = "telemetry"))]
    let _ = monitors;
}

/// An overlay finish succeeded: the action, the drawing tools used and the marks (counted, never sent).
pub fn finished(meta: &FinishMeta, marks: &[Mark]) {
    #[cfg(feature = "telemetry")]
    usage::finished(meta, marks);
    #[cfg(not(feature = "telemetry"))]
    let _ = (meta, marks);
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JsErrorKind {
    Error,
    UnhandledRejection,
}

/// A JavaScript error from a webview (window `error` or `unhandledrejection`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsErrorReport {
    pub kind: JsErrorKind,
    pub message: String,
    #[serde(default)]
    pub stack: Option<String>,
    /// `globalThis._sentryDebugIds`: script URL to debug id, for source maps.
    #[serde(default)]
    pub debug_ids: HashMap<String, String>,
}

/// `page` is the window label of the sender.
pub fn report_js_error(app: &AppHandle, page: &str, report: JsErrorReport) {
    #[cfg(feature = "telemetry")]
    crash::report_js_error(page, report);
    #[cfg(not(feature = "telemetry"))]
    let _ = (page, report);
    let _ = app;
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryStatus {
    /// Crash reports waiting in the outbox (for the "ask" prompt, offline, or before onboarding is done).
    pub pending_reports: u32,
}

#[cfg(feature = "telemetry")]
pub fn status(_app: &AppHandle) -> TelemetryStatus {
    TelemetryStatus { pending_reports: crash::pending_reports() }
}

#[cfg(not(feature = "telemetry"))]
pub fn status(_app: &AppHandle) -> TelemetryStatus {
    TelemetryStatus::default()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SentKind {
    Crash,
    Error,
    Usage,
}

/// One line of "Show what was sent".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SentItem {
    /// Unix seconds.
    pub at: i64,
    pub kind: SentKind,
    pub summary: String,
    /// The sent content as shown to the user (event title or the usage JSON).
    pub detail: Option<String>,
}

/// The most recent uploads, newest first.
#[cfg(feature = "telemetry")]
pub fn history(_app: &AppHandle) -> Vec<SentItem> {
    store::history()
}

/// The most recent uploads, newest first.
#[cfg(not(feature = "telemetry"))]
pub fn history(_app: &AppHandle) -> Vec<SentItem> {
    Vec::new()
}

/// What to do with the pending crash reports. The prompt card answers Send, DontSend or Always; Settings >
/// Privacy uses Send and Delete, which do not count as a "Don't send" answer.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CrashAnswer {
    Send,
    DontSend,
    Always,
    Delete,
}

pub fn crash_prompt_answer(app: &AppHandle, answer: CrashAnswer) {
    #[cfg(feature = "telemetry")]
    crash::crash_prompt_answer(app, answer);
    #[cfg(not(feature = "telemetry"))]
    let _ = (app, answer);
}

/// "Reset install number": new random ids for usage and crash reports.
pub fn reset_ids(app: &AppHandle) {
    let _ = app;
    #[cfg(feature = "telemetry")]
    crash::reset_ids();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_must_be_non_empty() {
        assert!(!is_key(None));
        assert!(!is_key(Some("")));
        assert!(!is_key(Some("  ")));
        assert!(is_key(Some("phc_x")));
        if !cfg!(feature = "telemetry") {
            assert!(!available(), "a build without the feature sends nothing");
        }
    }

    #[test]
    fn sending_needs_the_feature_and_both_keys() {
        let (dsn, key) = (Some("https://k@o1.ingest.de.sentry.io/2"), Some("phc_x"));
        assert!(configured(true, dsn, key));
        assert!(!configured(false, dsn, key));
        assert!(!configured(true, dsn, None), "crash reports only: the statistics switch would lie");
        assert!(!configured(true, None, key), "statistics only: the crash modes would lie");
        assert!(!configured(true, Some(""), key));
    }

    #[test]
    fn a_webview_reloads_at_most_three_times_a_minute() {
        use std::time::{Duration, Instant};
        let mut reloads = Vec::new();
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(allow_reload(&mut reloads, "editor", t0));
        }
        assert!(!allow_reload(&mut reloads, "editor", t0 + Duration::from_secs(30)), "a page that keeps dying");
        assert!(allow_reload(&mut reloads, "settings", t0 + Duration::from_secs(30)), "per webview");
        assert!(allow_reload(&mut reloads, "editor", t0 + Duration::from_secs(61)), "a minute later");
    }

    #[test]
    fn ipc_shapes_match_the_frontend() {
        let report: JsErrorReport =
            serde_json::from_str(r#"{"kind":"unhandledRejection","message":"boom","stack":null,"debugIds":{"app:///a.js":"d1"}}"#)
                .unwrap();
        assert_eq!(report.kind, JsErrorKind::UnhandledRejection);
        assert_eq!(report.debug_ids.get("app:///a.js").map(String::as_str), Some("d1"));
        let bare: JsErrorReport = serde_json::from_str(r#"{"kind":"error","message":"x"}"#).unwrap();
        assert!(bare.stack.is_none() && bare.debug_ids.is_empty());
        let answer: CrashAnswer = serde_json::from_str(r#""dontSend""#).unwrap();
        assert_eq!(answer, CrashAnswer::DontSend);
        let item = SentItem { at: 5, kind: SentKind::Usage, summary: "usage_daily".into(), detail: None };
        assert_eq!(
            serde_json::to_value(item).unwrap(),
            serde_json::json!({"at": 5, "kind": "usage", "summary": "usage_daily", "detail": null})
        );
        assert_eq!(serde_json::to_value(TelemetryStatus::default()).unwrap(), serde_json::json!({"pendingReports": 0}));
    }
}
