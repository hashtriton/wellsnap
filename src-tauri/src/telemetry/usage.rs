//! Anonymous usage statistics: counters for the current local day, kept in telemetry.json and turned into
//! one `usage_daily` event when the day is over, plus a few one-off events. Every name is in events.json;
//! anything else is refused. Never: pixels, clipboard, comment text, paths or file names, window titles, the
//! target app's name, user or host names, the IP (PostHog derives the country from it and discards it).

use super::store;
use crate::actions::{FinishAction, FinishMeta};
use crate::capture::MonitorInfo;
use crate::session::Mark;
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Statistics are counted in this run: a sending build, active telemetry and the setting on.
pub static ON: AtomicBool = AtomicBool::new(false);
/// Days and events waiting to be sent; the oldest go first when there are more.
const MAX_PENDING: usize = 45;
/// The core's own metric: hotkey to visible overlay (overlay.rs `on_ready`).
pub const READY_METRIC: &str = "hotkeyToVisibleMs";
const BUCKET_LIMITS: [f64; 4] = [100.0, 200.0, 400.0, 800.0];
const READY_BUCKETS: [&str; 5] =
    ["ready_ms_lt100", "ready_ms_lt200", "ready_ms_lt400", "ready_ms_lt800", "ready_ms_ge800"];
const CAPTURE_BUCKETS: [&str; 5] =
    ["capture_ms_lt100", "capture_ms_lt200", "capture_ms_lt400", "capture_ms_lt800", "capture_ms_ge800"];
const SCALE_STEPS: [u64; 6] = [100, 125, 150, 175, 200, 250];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageData {
    /// Local date (YYYY-MM-DD) the counters belong to.
    pub day: String,
    pub counts: BTreeMap<String, u64>,
    /// Highest value of the day (monitors, scale, set size).
    pub gauges: BTreeMap<String, u64>,
    pub pending: Vec<PendingEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PendingEvent {
    /// PostHog drops a second event with the same uuid, so a retried upload is not counted twice.
    pub uuid: String,
    pub event: String,
    /// ISO 8601, UTC.
    pub timestamp: String,
    pub properties: Map<String, Value>,
}

struct Catalog {
    events: BTreeMap<String, Vec<String>>,
    /// The property lists are checked against the built events by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    common: HashSet<String>,
    counters: HashSet<String>,
    gauges: HashSet<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    set: HashSet<String>,
}

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let json: Value = serde_json::from_str(include_str!("events.json")).expect("events.json is valid JSON");
        let names = |key: &str| -> HashSet<String> {
            json[key].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
        };
        let events = json["events"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, props)| {
                let props = props.as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_string));
                (name.clone(), props.collect())
            })
            .collect();
        Catalog {
            events,
            common: names("common"),
            counters: names("counters"),
            gauges: names("gauges"),
            set: names("set"),
        }
    })
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Adds `n` to a counter of today.
pub fn add(name: &'static str, n: u64) {
    if !ON.load(Ordering::Relaxed) || n == 0 {
        return;
    }
    if !catalog().counters.contains(name) {
        debug_assert!(false, "usage counter {name} is not in events.json");
        return;
    }
    let today = today();
    store::update_quiet(|s| {
        roll(&mut s.usage, &today);
        *s.usage.counts.entry(name.to_string()).or_default() += n;
    });
}

/// Keeps the highest value of the day.
fn gauge(name: &'static str, value: u64) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    if !catalog().gauges.contains(name) {
        debug_assert!(false, "usage gauge {name} is not in events.json");
        return;
    }
    let today = today();
    store::update_quiet(|s| {
        roll(&mut s.usage, &today);
        let slot = s.usage.gauges.entry(name.to_string()).or_default();
        *slot = (*slot).max(value);
    });
}

/// A new local day: yesterday's counters become a pending `usage_daily` event. A clock that went back keeps
/// counting into the newer day, so no day is reported twice. True when a day was closed.
pub fn roll(data: &mut UsageData, today: &str) -> bool {
    if data.day.is_empty() {
        data.day = today.to_string();
        return false;
    }
    if today <= data.day.as_str() {
        return false;
    }
    let day = std::mem::replace(&mut data.day, today.to_string());
    let counts = std::mem::take(&mut data.counts);
    let gauges = std::mem::take(&mut data.gauges);
    if counts.is_empty() && gauges.is_empty() {
        return true;
    }
    let mut properties = Map::new();
    properties.insert("day".into(), json!(day));
    properties.extend(counts.into_iter().map(|(k, v)| (k, json!(v))));
    properties.extend(gauges.into_iter().map(|(k, v)| (k, json!(v))));
    let event = PendingEvent {
        uuid: store::new_id(),
        event: "usage_daily".into(),
        timestamp: format!("{day}T12:00:00Z"),
        properties,
    };
    push(data, event);
    true
}

fn push(data: &mut UsageData, event: PendingEvent) {
    data.pending.push(event);
    let excess = data.pending.len().saturating_sub(MAX_PENDING);
    data.pending.drain(..excess);
}

/// Closes the day if the date changed (the uploader calls this every tick, so midnight closes it too).
pub fn roll_now() {
    let today = today();
    store::update_quiet(|s| roll(&mut s.usage, &today));
}

/// A one-off event (`stats_enabled`, `onboarding_finished`, `app_updated`) with properties from events.json.
pub fn queue(data: &mut UsageData, event: &str, properties: Map<String, Value>) {
    debug_assert!(
        catalog().events.get(event).is_some_and(|allowed| properties.keys().all(|k| allowed.contains(k))),
        "{event} or one of its properties is not in events.json"
    );
    // With no relay in between, release builds enforce the list here too.
    let Some(properties) = listed(event, properties) else { return };
    push(data, PendingEvent { uuid: store::new_id(), event: event.into(), timestamp: now_iso(), properties });
}

/// The properties events.json lists for `event`; None for an event it does not list.
fn listed(event: &str, properties: Map<String, Value>) -> Option<Map<String, Value>> {
    let allowed = catalog().events.get(event)?;
    Some(properties.into_iter().filter(|(key, _)| allowed.contains(key)).collect())
}

/// `log_metric` names: `captureMs(<transport>)` from the overlay, and the core's `READY_METRIC`.
pub fn metric(_page: &str, name: &str, value: f64) {
    if !value.is_finite() || value < 0.0 {
        return;
    }
    if name.starts_with("captureMs") {
        add(bucket(&CAPTURE_BUCKETS, value), 1);
    } else if name == READY_METRIC {
        add(bucket(&READY_BUCKETS, value), 1);
    }
}

fn bucket(names: &[&'static str; 5], ms: f64) -> &'static str {
    let index = BUCKET_LIMITS.iter().position(|limit| ms < *limit).unwrap_or(BUCKET_LIMITS.len());
    names[index]
}

pub fn monitors_seen(monitors: &[MonitorInfo]) {
    gauge("monitors", monitors.len().min(3) as u64);
    let max_scale = monitors.iter().map(|m| m.scale).fold(0.0, f64::max);
    if let Some(step) = scale_bucket(max_scale) {
        gauge("max_scale", step);
    }
}

/// Display scale as the nearest Windows step at or below it: 100, 125, 150, 175, 200, 250 (and above).
fn scale_bucket(scale: f64) -> Option<u64> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let percent = (scale * 100.0).round() as u64;
    Some(SCALE_STEPS.iter().rev().find(|step| percent >= **step).copied().unwrap_or(SCALE_STEPS[0]))
}

pub fn finished(meta: &FinishMeta, marks: &[Mark]) {
    add(
        match meta.action {
            FinishAction::Copy => "finish_copy",
            FinishAction::Save => "finish_save",
            FinishAction::AddToSet => "finish_add_to_set",
            FinishAction::CopyAll => "finish_copy_all",
            FinishAction::Pin => "finish_pin",
        },
        1,
    );
    let mut annotated = false;
    for (tool, n) in &meta.tools {
        if let Some(counter) = tool_counter(tool) {
            add(counter, u64::from(*n));
            annotated |= *n > 0;
        }
    }
    if annotated {
        add("captures_annotated", 1);
    }
    let with_comment = marks.iter().filter(|m| !m.comment.trim().is_empty()).count() as u64;
    add("marks_total", marks.len() as u64);
    add("marks_with_comment", with_comment);
    if with_comment > 0 && !meta.comments_on_image {
        add("comments_separate", 1);
    }
    // The letter drawn on a set snapshot is its place in the set: C is the third.
    if matches!(meta.action, FinishAction::AddToSet | FinishAction::CopyAll)
        && let Some(letter) = meta.letter.filter(char::is_ascii_uppercase)
    {
        gauge("set_max_size", u64::from(letter as u8 - b'A') + 1);
    }
}

/// Only the overlay's DRAW_TOOLS: the keys come from a webview.
fn tool_counter(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "arrow" => "tool_arrow",
        "pencil" => "tool_pencil",
        "rect" => "tool_rect",
        "marker" => "tool_marker",
        "blur" => "tool_blur",
        "mark" => "tool_mark",
        "text" => "tool_text",
        _ => return None,
    })
}

/// Properties on every event.
#[derive(Debug, Clone, Default)]
pub struct Common {
    pub app_version: String,
    pub os_version: String,
    pub webview_major: Option<String>,
}

pub fn common_properties(common: &Common, settings: &Settings) -> Map<String, Value> {
    let os = if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "other" };
    let lang = crate::i18n::Lang::resolve(settings.language);
    let mut props = Map::new();
    props.insert("app_version".into(), json!(common.app_version));
    props.insert("os".into(), json!(os));
    props.insert("os_version".into(), json!(common.os_version));
    props.insert("arch".into(), json!(arch()));
    props.insert("ui_lang".into(), serde_json::to_value(lang).unwrap_or(Value::Null));
    props.insert("lang_pref".into(), serde_json::to_value(settings.language).unwrap_or(Value::Null));
    props.insert("install_week".into(), json!(install_week(settings.donations.first_run_at)));
    props.insert("webview_major".into(), json!(common.webview_major));
    props.insert("$lib".into(), json!("wellsnap"));
    props
}

pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// ISO week of the first run, e.g. "2026-W43": cohorts without a date that could identify anyone.
fn install_week(first_run_at: i64) -> Option<String> {
    (first_run_at > 0)
        .then(|| chrono::DateTime::from_timestamp(first_run_at, 0))
        .flatten()
        .map(|t| t.format("%G-W%V").to_string())
}

/// "11.26100" on Windows (major plus build), "15" on macOS (major only). `version` as the OS context reports
/// it: "10.0.26100" or "15.1.0".
pub fn os_version_label(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    if cfg!(windows) {
        let build = parts.get(2).and_then(|b| b.parse::<u32>().ok());
        return match build {
            Some(build) => format!("{}.{build}", if build >= 22000 { 11 } else { 10 }),
            None => parts.first().copied().unwrap_or("").to_string(),
        };
    }
    parts.first().copied().unwrap_or("").to_string()
}

/// Person properties sent with `usage_daily` (`$set`): the settings as categories and switches, never values
/// such as paths or hotkey strings.
pub fn set_properties(settings: &Settings) -> Map<String, Value> {
    let mut props = Map::new();
    let mut put = |key: &str, value: Value| {
        props.insert(key.to_string(), value);
    };
    put("theme", serde_json::to_value(settings.theme).unwrap_or_default());
    put("image_format", serde_json::to_value(settings.image_format).unwrap_or_default());
    put("enter_action", serde_json::to_value(settings.enter_action).unwrap_or_default());
    put("comment_placement", serde_json::to_value(settings.comment_placement).unwrap_or_default());
    put("show_hints", json!(settings.show_hints));
    put("hide_strip", json!(settings.hide_strip_from_capture));
    put("autostart", json!(settings.autostart));
    put("auto_check_updates", json!(settings.auto_check_updates));
    put("auto_install_updates", json!(settings.auto_install_updates));
    put("save_dir_custom", json!(settings.save_dir.is_some()));
    put("print_screen_taken_over", json!(settings.print_screen_taken_over));
    put("set_copy_mode", serde_json::to_value(settings.set_copy_mode).unwrap_or_default());
    put("crash_reports", serde_json::to_value(settings.crash_reports).unwrap_or_default());
    put(
        "hotkey_capture_default",
        json!(settings.capture_hotkey.eq_ignore_ascii_case(crate::settings::default_capture_hotkey())),
    );
    put(
        "hotkey_paste_default",
        json!(settings.smart_paste_hotkey.eq_ignore_ascii_case(crate::settings::default_smart_paste_hotkey())),
    );
    props
}

/// The PostHog `/batch/` body: `usage_daily` carries the settings snapshot in `$set`.
pub fn batch_body(
    api_key: &str,
    distinct_id: &str,
    events: &[PendingEvent],
    common: &Map<String, Value>,
    set: &Map<String, Value>,
) -> Value {
    let batch: Vec<Value> = events
        .iter()
        .map(|e| {
            let mut properties = common.clone();
            properties.extend(e.properties.clone());
            if e.event == "usage_daily" {
                properties.insert("$set".into(), Value::Object(set.clone()));
            }
            json!({
                "event": e.event,
                "distinct_id": distinct_id,
                "uuid": e.uuid,
                "timestamp": e.timestamp,
                "properties": properties,
            })
        })
        .collect();
    json!({ "api_key": api_key, "batch": batch })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_day_closes_the_old_one() {
        let mut data = UsageData::default();
        assert!(!roll(&mut data, "2026-10-21"), "first use only sets the day");
        data.counts.insert("launches".into(), 2);
        data.gauges.insert("monitors".into(), 2);
        assert!(!roll(&mut data, "2026-10-21"));
        assert!(roll(&mut data, "2026-10-22"));
        assert_eq!(data.day, "2026-10-22");
        assert!(data.counts.is_empty() && data.gauges.is_empty());
        let event = &data.pending[0];
        assert_eq!(event.event, "usage_daily");
        assert_eq!(event.timestamp, "2026-10-21T12:00:00Z");
        assert_eq!(Value::Object(event.properties.clone()), json!({"day": "2026-10-21", "launches": 2, "monitors": 2}));
        // An idle day sends nothing.
        assert!(roll(&mut data, "2026-10-25"));
        assert_eq!(data.pending.len(), 1);
    }

    #[test]
    fn a_clock_going_back_keeps_the_newer_day() {
        let mut data = UsageData { day: "2026-10-22".into(), ..Default::default() };
        data.counts.insert("launches".into(), 1);
        assert!(!roll(&mut data, "2026-10-20"));
        assert_eq!(data.day, "2026-10-22");
        assert_eq!(data.counts["launches"], 1);
        assert!(data.pending.is_empty());
    }

    #[test]
    fn pending_events_are_capped_oldest_first() {
        let mut data = UsageData::default();
        for i in 0..(MAX_PENDING + 5) {
            push(&mut data, PendingEvent { event: format!("e{i}"), ..Default::default() });
        }
        assert_eq!(data.pending.len(), MAX_PENDING);
        assert_eq!(data.pending[0].event, "e5");
    }

    #[test]
    fn unlisted_events_and_properties_are_dropped() {
        let props = Map::from_iter([
            ("source".to_string(), json!("settings")),
            ("save_dir".to_string(), json!(r"C:\Users\anna\Shots")),
        ]);
        let kept = Map::from_iter([("source".to_string(), json!("settings"))]);
        assert_eq!(listed("stats_enabled", props.clone()), Some(kept));
        assert_eq!(listed("file_saved", props), None);
    }

    #[test]
    fn timing_and_scale_buckets() {
        assert_eq!(bucket(&READY_BUCKETS, 0.0), "ready_ms_lt100");
        assert_eq!(bucket(&READY_BUCKETS, 99.9), "ready_ms_lt100");
        assert_eq!(bucket(&READY_BUCKETS, 100.0), "ready_ms_lt200");
        assert_eq!(bucket(&CAPTURE_BUCKETS, 799.0), "capture_ms_lt800");
        assert_eq!(bucket(&CAPTURE_BUCKETS, 5000.0), "capture_ms_ge800");
        assert_eq!(scale_bucket(1.0), Some(100));
        assert_eq!(scale_bucket(1.25), Some(125));
        assert_eq!(scale_bucket(1.4), Some(125));
        assert_eq!(scale_bucket(2.0), Some(200));
        assert_eq!(scale_bucket(3.0), Some(250));
        assert_eq!(scale_bucket(0.9), Some(100));
        assert_eq!(scale_bucket(f64::NAN), None);
    }

    #[test]
    fn tool_keys_from_the_webview_are_checked() {
        for tool in ["arrow", "pencil", "rect", "marker", "blur", "mark", "text"] {
            let counter = tool_counter(tool).unwrap();
            assert!(catalog().counters.contains(counter), "{counter}");
        }
        assert_eq!(tool_counter("C:\\x"), None);
        assert_eq!(tool_counter("ARROW"), None);
    }

    #[test]
    fn os_versions_are_major_and_build_only() {
        if cfg!(windows) {
            assert_eq!(os_version_label("10.0.26100"), "11.26100");
            assert_eq!(os_version_label("10.0.19045"), "10.19045");
            assert_eq!(os_version_label("10"), "10");
        } else {
            assert_eq!(os_version_label("15.1.0"), "15");
        }
        assert_eq!(install_week(0), None);
        assert_eq!(install_week(1_761_134_400).as_deref(), Some("2025-W43"));
    }

    #[test]
    fn every_counter_the_code_uses_is_in_events_json() {
        let sources = [
            include_str!("../actions.rs"),
            include_str!("../overlay.rs"),
            include_str!("../hotkeys.rs"),
            include_str!("../commands.rs"),
            include_str!("crash.rs"),
            include_str!("usage.rs"),
        ];
        let mut found = 0;
        for source in sources {
            for (i, _) in source.match_indices("count(\"") {
                let rest = &source[i + 7..];
                let name = &rest[..rest.find('"').unwrap()];
                assert!(catalog().counters.contains(name), "count(\"{name}\") is not in events.json");
                found += 1;
            }
            for (i, _) in source.match_indices("add(\"") {
                let rest = &source[i + 5..];
                let name = &rest[..rest.find('"').unwrap()];
                assert!(catalog().counters.contains(name), "add(\"{name}\") is not in events.json");
            }
        }
        assert!(found >= 5, "the scan found the counter calls ({found})");
        for name in READY_BUCKETS.iter().chain(&CAPTURE_BUCKETS) {
            assert!(catalog().counters.contains(*name), "{name}");
        }
        for name in ["finish_copy", "finish_save", "finish_add_to_set", "finish_copy_all", "captures_annotated"] {
            assert!(catalog().counters.contains(name), "{name}");
        }
    }

    #[test]
    fn the_batch_holds_only_listed_keys() {
        let settings = Settings { save_dir: Some(r"C:\Users\anna\Shots".into()), ..Default::default() };
        let common = common_properties(
            &Common { app_version: "0.1.0".into(), os_version: "11.26100".into(), webview_major: Some("141".into()) },
            &settings,
        );
        let set = set_properties(&settings);
        let mut data = UsageData { day: "2026-10-21".into(), ..Default::default() };
        data.counts.insert("finish_copy".into(), 3);
        roll(&mut data, "2026-10-22");
        queue(&mut data, "stats_enabled", Map::from_iter([("source".to_string(), json!("default"))]));
        let body = batch_body("phc_key", "id-1", &data.pending, &common, &set);
        assert_eq!(body["api_key"], "phc_key");
        let catalog = catalog();
        for event in body["batch"].as_array().unwrap() {
            assert_eq!(event["distinct_id"], "id-1");
            let name = event["event"].as_str().unwrap();
            let allowed = &catalog.events[name];
            for key in event["properties"].as_object().unwrap().keys() {
                let ok = catalog.common.contains(key)
                    || allowed.contains(key)
                    || (name == "usage_daily"
                        && (catalog.counters.contains(key) || catalog.gauges.contains(key) || key == "$set"));
                assert!(ok, "{name}.{key} is not in events.json");
            }
            if let Some(set) = event["properties"].get("$set") {
                for key in set.as_object().unwrap().keys() {
                    assert!(catalog.set.contains(key), "$set.{key}");
                }
            }
        }
        let text = body.to_string();
        assert!(!text.contains("anna") && !text.contains("Shots"), "no paths: {text}");
        assert_eq!(set["save_dir_custom"], true);
        assert!(set.keys().all(|k| catalog.set.contains(k)) && set.len() == catalog.set.len());
    }
}
