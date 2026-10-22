//! Crash reports (Sentry) and anonymous daily usage totals (PostHog), sent directly by the app. Only a build
//! with the "telemetry" cargo feature and the keys given at build time (WELLSNAP_SENTRY_DSN,
//! WELLSNAP_POSTHOG_KEY, optional WELLSNAP_POSTHOG_HOST) can send anything; forks and local builds stay
//! silent. Nothing goes out before onboarding is done, and the settings `crash_reports` and `usage_stats`
//! turn either part off. Screenshots, clipboard, comments, file names and window titles are never sent.
//!
//! Contract only: every function below is a no-op until the telemetry package fills it in.
#![allow(dead_code)] // Report fields are read once the bodies exist.

use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::AppHandle;

/// Held for the whole run (crash handler and reporter process).
pub struct Guard;

/// First thing in `run()`, before Tauri and single-instance: the crash reporter process re-runs the same
/// binary and never returns from here.
pub fn early_init() -> Guard {
    Guard
}

/// Extra log target: errors from WellSnap's own code become reports, other lines become breadcrumbs.
pub fn log_dispatch() -> tauri_plugin_log::fern::Dispatch {
    tauri_plugin_log::fern::Dispatch::new()
}

/// In `setup`, once `AppState` exists: crash prompt, uploader and usage counters. An install that finished
/// onboarding in a version without telemetry never saw the onboarding card: telemetry.json does not exist
/// yet then, and the switches must be shown once before anything is sent.
pub fn start(_app: &AppHandle) {}

/// `RunEvent::Exit`: marks the run as a clean exit.
pub fn shutdown() {}

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
pub fn settings_changed(_app: &AppHandle, _before: &Settings, _after: &Settings) {}

/// The onboarding window closed (finished or dismissed); sending may start. Called on every close: on macOS
/// the window opens again when screen permission is missing, and `onboarding_done` is already true when the
/// "Done" button closes it, so one-time work (the `onboarding_finished` event) must be recorded in
/// telemetry.json and done once.
pub fn onboarding_finished(_app: &AppHandle) {}

/// macOS killed a webview's WebContent process (the page is blank until it loads again). Registered in
/// `lib.rs`; WKWebView does nothing on its own, so the report and the reload both belong here.
#[cfg(target_os = "macos")]
pub fn web_content_terminated(_webview: &tauri::Webview<tauri::Wry>) {}

/// Adds one to a daily usage counter (names as in telemetry/events.json).
pub fn count(_name: &'static str) {}

/// A timing or size reported by a webview through `log_metric`.
pub fn metric(_page: &str, _name: &str, _value: f64) {}

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
pub fn report_js_error(_app: &AppHandle, _page: &str, _report: JsErrorReport) {}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryStatus {
    /// Crash reports waiting in the outbox (for the "ask" prompt, offline, or before onboarding is done).
    pub pending_reports: u32,
}

pub fn status(_app: &AppHandle) -> TelemetryStatus {
    TelemetryStatus::default()
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SentKind {
    Crash,
    Error,
    Usage,
}

/// One line of "Show what was sent".
#[derive(Debug, Clone, Serialize)]
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

pub fn crash_prompt_answer(_app: &AppHandle, _answer: CrashAnswer) {}

/// "Reset install number": new random ids for usage and crash reports.
pub fn reset_ids(_app: &AppHandle) {}

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
