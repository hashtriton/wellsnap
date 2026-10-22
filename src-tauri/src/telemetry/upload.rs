//! The one thread that sends: outbox files to the Sentry ingest from the DSN (`/api/<project>/envelope/` with
//! `X-Sentry-Auth`) and pending usage events to PostHog `{host}/batch/`. No relay: the keys are the build's
//! public ingest keys. Starts 20 s after launch, then runs every 10 minutes or when woken (a new consent,
//! "Send" on the crash card). Nothing goes out before onboarding is done and the switches were seen.
//!
//! Replies: 2xx sent (the file or events are deleted and logged in sent.jsonl), 429 waits for Retry-After,
//! any other 4xx is a request that will never succeed (dropped), network errors and 5xx back off from one
//! minute up to six hours.

use super::outbox;
use super::store;
use super::usage::{self, Common};
use super::{SentItem, SentKind};
use crate::settings::CrashMode;
use crate::state::AppState;
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const FIRST_DELAY: Duration = Duration::from_secs(20);
const INTERVAL: Duration = Duration::from_secs(10 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Events per PostHog request.
const BATCH_MAX: usize = 50;
const DEFAULT_RETRY_AFTER: u64 = 60;
const DEFAULT_POSTHOG_HOST: &str = "https://us.i.posthog.com";

static WOKEN: Mutex<bool> = Mutex::new(false);
static WAKE: Condvar = Condvar::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Sent,
    /// A 4xx other than 429: the same request would fail again.
    Rejected,
    RetryAfter(u64),
    Backoff,
}

pub fn verdict(status: Option<u16>, retry_after: Option<&str>) -> Verdict {
    match status {
        Some(200..=299) => Verdict::Sent,
        Some(429) => {
            let secs = retry_after.and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_RETRY_AFTER);
            Verdict::RetryAfter(secs.clamp(1, 24 * 60 * 60))
        }
        Some(400..=499) => Verdict::Rejected,
        _ => Verdict::Backoff,
    }
}

pub fn batch_url(host: Option<&str>) -> String {
    let host = host.map(str::trim).filter(|h| !h.is_empty()).unwrap_or(DEFAULT_POSTHOG_HOST);
    format!("{}/batch/", host.trim_end_matches('/'))
}

/// Runs the next round now.
pub fn wake() {
    *WOKEN.lock().unwrap_or_else(|e| e.into_inner()) = true;
    WAKE.notify_all();
}

fn sleep(timeout: Duration) {
    let guard = WOKEN.lock().unwrap_or_else(|e| e.into_inner());
    let (mut woken, _) = WAKE.wait_timeout_while(guard, timeout, |woken| !*woken).unwrap_or_else(|e| e.into_inner());
    *woken = false;
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn user_agent() -> String {
    format!("wellsnap/{}", env!("CARGO_PKG_VERSION"))
}

pub fn start(app: AppHandle, common: Common) {
    std::thread::spawn(move || run(&app, &common));
}

fn run(app: &AppHandle, common: &Common) {
    // reqwest is built with `rustls-no-provider`; the updater installs ring only inside its own check, which
    // the user can turn off. Installing it twice is harmless.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).user_agent(user_agent()).build() {
        Ok(client) => client,
        Err(err) => {
            log::warn!("telemetry uploader not started: {err}");
            return;
        }
    };
    sleep(FIRST_DELAY);
    loop {
        tick(app, &client, common);
        store::flush();
        sleep(INTERVAL);
    }
}

fn tick(app: &AppHandle, client: &reqwest::Client, common: &Common) {
    usage::roll_now();
    let settings = app.state::<AppState>().settings();
    let (Some(state), Some(dirs)) = (store::get(), store::dirs()) else { return };
    if !gate_open(settings.onboarding_done, state.consent_seen) {
        return;
    }
    let now = now_secs();
    if settings.crash_reports == CrashMode::Never {
        outbox::purge(&dirs.outbox());
    } else if state.crash_retry.ready(now) {
        // A "Send" in "ask" mode stays in telemetry.json, so a failed upload (offline, 5xx, 429) tries again later.
        let files: Vec<_> = outbox::list(&dirs.outbox())
            .into_iter()
            .filter(|f| outbox::sendable(f, settings.crash_reports, state.crash_approved_until))
            .collect();
        send_outbox(client, &files, now);
    }
    let usage_due = settings.usage_stats
        && usage::ON.load(Ordering::Relaxed)
        && state.usage_id.is_some()
        && !state.usage.pending.is_empty()
        && state.usage_retry.ready(now);
    if usage_due {
        let common = usage::common_properties(common, &settings);
        let set = usage::set_properties(&settings);
        send_usage(client, &common, &set, now);
    }
}

/// The HTTP status (None when the request did not get through) and Retry-After.
fn post(
    client: &reqwest::Client,
    url: &str,
    headers: &[(&str, String)],
    body: Vec<u8>,
) -> (Option<u16>, Option<String>) {
    let mut request = client.post(url).body(body);
    for (name, value) in headers {
        request = request.header(*name, value.as_str());
    }
    tauri::async_runtime::block_on(async move {
        match request.send().await {
            Ok(response) => {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                (Some(response.status().as_u16()), retry_after)
            }
            Err(err) => {
                log::info!("telemetry upload failed: {}", err.without_url());
                (None, None)
            }
        }
    })
}

/// Nothing goes out before onboarding is done and the switches were seen.
fn gate_open(onboarding_done: bool, consent_seen: bool) -> bool {
    onboarding_done && consent_seen
}

fn send_outbox(client: &reqwest::Client, files: &[outbox::OutboxFile], now: i64) {
    if files.is_empty() {
        return;
    }
    let Some(dsn) = option_env!("WELLSNAP_SENTRY_DSN").and_then(|d| d.trim().parse::<sentry::types::Dsn>().ok()) else {
        return;
    };
    let url = dsn.envelope_api_url().to_string();
    let headers = [
        ("X-Sentry-Auth", dsn.to_auth(Some(&user_agent())).to_string()),
        ("Content-Type", "application/x-sentry-envelope".to_string()),
    ];
    for file in files {
        let Ok(bytes) = std::fs::read(&file.path) else { continue };
        let (status, retry_after) = post(client, &url, &headers, bytes.clone());
        match verdict(status, retry_after.as_deref()) {
            Verdict::Sent => {
                let _ = std::fs::remove_file(&file.path);
                store::record_sent(&outbox::describe(&bytes, now));
                store::update(|s| s.crash_retry.succeeded());
            }
            Verdict::Rejected => {
                let _ = std::fs::remove_file(&file.path);
                log::warn!("Sentry refused a report (HTTP {}), dropped", status.unwrap_or(0));
            }
            Verdict::RetryAfter(secs) => {
                store::update(|s| s.crash_retry.wait(now, secs));
                return;
            }
            Verdict::Backoff => {
                store::update(|s| s.crash_retry.back_off(now));
                return;
            }
        }
    }
}

fn send_usage(
    client: &reqwest::Client,
    common: &serde_json::Map<String, serde_json::Value>,
    set: &serde_json::Map<String, serde_json::Value>,
    now: i64,
) {
    let Some(key) = option_env!("WELLSNAP_POSTHOG_KEY").map(str::trim).filter(|k| !k.is_empty()) else { return };
    let Some(state) = store::get() else { return };
    let Some(id) = state.usage_id else { return };
    let events: Vec<_> = state.usage.pending.iter().take(BATCH_MAX).cloned().collect();
    let body = usage::batch_body(key, &id, &events, common, set);
    let Ok(bytes) = serde_json::to_vec(&body) else { return };
    let headers = [("Content-Type", "application/json".to_string())];
    let (status, retry_after) = post(client, &batch_url(option_env!("WELLSNAP_POSTHOG_HOST")), &headers, bytes);
    let verdict = verdict(status, retry_after.as_deref());
    let done: Vec<String> = events.iter().map(|e| e.uuid.clone()).collect();
    match verdict {
        Verdict::Sent | Verdict::Rejected => {
            store::update(|s| {
                s.usage.pending.retain(|e| !done.contains(&e.uuid));
                s.usage_retry.succeeded();
            });
            if verdict == Verdict::Sent {
                store::record_sent(&usage_item(&body, now));
            } else {
                log::warn!("PostHog refused the usage events (HTTP {}), dropped", status.unwrap_or(0));
            }
        }
        Verdict::RetryAfter(secs) => {
            store::update(|s| s.usage_retry.wait(now, secs));
        }
        Verdict::Backoff => {
            store::update(|s| s.usage_retry.back_off(now));
        }
    }
}

/// The history line: event names (and the day of a daily total), and the body without the project key.
fn usage_item(body: &serde_json::Value, at: i64) -> SentItem {
    let batch = body["batch"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = batch
        .iter()
        .map(|e| match e["properties"]["day"].as_str() {
            Some(day) => format!("{} {day}", e["event"].as_str().unwrap_or("")),
            None => e["event"].as_str().unwrap_or("").to_string(),
        })
        .collect();
    let detail = serde_json::to_string_pretty(&serde_json::Value::Array(batch)).unwrap_or_default();
    SentItem { at, kind: SentKind::Usage, summary: names.join(", "), detail: Some(detail) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replies_decide_what_happens_to_a_report() {
        assert_eq!(verdict(Some(200), None), Verdict::Sent);
        assert_eq!(verdict(Some(204), None), Verdict::Sent);
        assert_eq!(verdict(Some(429), Some("120")), Verdict::RetryAfter(120));
        assert_eq!(verdict(Some(429), Some("Wed, 21 Oct 2026 07:28:00 GMT")), Verdict::RetryAfter(DEFAULT_RETRY_AFTER));
        assert_eq!(verdict(Some(429), None), Verdict::RetryAfter(DEFAULT_RETRY_AFTER));
        assert_eq!(verdict(Some(429), Some("999999")), Verdict::RetryAfter(24 * 60 * 60));
        assert_eq!(verdict(Some(400), None), Verdict::Rejected);
        assert_eq!(verdict(Some(413), None), Verdict::Rejected);
        assert_eq!(verdict(Some(500), None), Verdict::Backoff);
        assert_eq!(verdict(Some(503), Some("30")), Verdict::Backoff);
        assert_eq!(verdict(None, None), Verdict::Backoff, "offline");
        assert_eq!(verdict(Some(301), None), Verdict::Backoff);
    }

    #[test]
    fn nothing_goes_out_before_onboarding_and_the_switches() {
        assert!(!gate_open(false, false));
        assert!(!gate_open(true, false), "an older install that has not seen the Privacy tab yet");
        assert!(!gate_open(false, true));
        assert!(gate_open(true, true));
    }

    #[test]
    fn posthog_url_from_the_build_host() {
        assert_eq!(batch_url(None), "https://us.i.posthog.com/batch/");
        assert_eq!(batch_url(Some("")), "https://us.i.posthog.com/batch/", "an empty CI secret");
        assert_eq!(batch_url(Some("https://eu.i.posthog.com/")), "https://eu.i.posthog.com/batch/");
    }

    #[test]
    fn sentry_ingest_from_the_dsn() {
        let dsn: sentry::types::Dsn = "https://abc@o1.ingest.de.sentry.io/42".parse().unwrap();
        assert_eq!(dsn.envelope_api_url().as_str(), "https://o1.ingest.de.sentry.io/api/42/envelope/");
        let auth = dsn.to_auth(Some("wellsnap/0.1.0")).to_string();
        assert!(auth.starts_with("Sentry sentry_key=abc, sentry_version=7"), "{auth}");
        assert!(auth.contains("sentry_client=wellsnap/0.1.0"), "{auth}");
    }

    #[test]
    fn the_history_never_shows_the_project_key() {
        let body = json!({"api_key": "phc_secret", "batch": [
            {"event": "usage_daily", "properties": {"day": "2026-10-21"}},
            {"event": "stats_enabled", "properties": {"source": "settings"}},
        ]});
        let item = usage_item(&body, 9);
        assert_eq!(item.summary, "usage_daily 2026-10-21, stats_enabled");
        assert!(!item.detail.unwrap().contains("phc_secret"));
    }
}
