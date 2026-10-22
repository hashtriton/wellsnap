//! telemetry.json in the app config dir: the random ids, consent and session flags, client-side limits,
//! retry state and the usage counters. Kept apart from settings.json, which the webviews read and write.
//! Also sent.jsonl, the last uploads for "Show what was sent".
//!
//! Nothing here logs while the lock is held: the log target (crash::on_log) takes the same lock for its
//! limits, and a log line from inside would lock it twice on one thread.

use super::paths::Dirs;
use super::usage::UsageData;
use super::SentItem;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Non-fatal reports (log errors, JavaScript errors) per install and day.
pub const MAX_NONFATAL_PER_DAY: u32 = 30;
/// The same error is reported again only after a day.
const SAME_ERROR_AGAIN_AFTER: i64 = 24 * 60 * 60;
/// Entries kept in sent.jsonl.
const SENT_LOG_MAX: usize = 50;
const BACKOFF_FIRST: u64 = 60;
const BACKOFF_MAX: u64 = 6 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    /// Sentry `user.id`: lets Sentry count affected installs. Random, never derived from the machine.
    pub crash_id: Option<String>,
    /// PostHog `distinct_id`; exists only while statistics are on.
    pub usage_id: Option<String>,
    /// The user has seen the switches (onboarding card, or Settings > Privacy for an older install).
    pub consent_seen: bool,
    /// Set at start, cleared on a clean exit: still set at the next start means the app died.
    pub session_open: bool,
    /// Version of the previous run (an update is not an unclean exit, and is reported once).
    pub last_version: Option<String>,
    pub onboarding_event_sent: bool,
    /// "Don't send" answers in a row; 3 switch the crash reports to "never".
    pub dont_send_streak: u32,
    pub crash_retry: Retry,
    pub usage_retry: Retry,
    pub limits: Limits,
    pub usage: UsageData,
}

/// When a failed or rate-limited upload may run again (unix seconds).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Retry {
    pub not_before: i64,
    pub backoff_secs: u64,
}

impl Retry {
    pub fn ready(&self, now: i64) -> bool {
        now >= self.not_before
    }

    pub fn succeeded(&mut self) {
        *self = Retry::default();
    }

    /// The server said when (429 with Retry-After).
    pub fn wait(&mut self, now: i64, secs: u64) {
        self.not_before = now + secs as i64;
    }

    /// Offline or a server error: 1 minute, doubling up to 6 hours.
    pub fn back_off(&mut self, now: i64) {
        self.backoff_secs =
            if self.backoff_secs == 0 { BACKOFF_FIRST } else { (self.backoff_secs * 2).min(BACKOFF_MAX) };
        self.not_before = now + self.backoff_secs as i64;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Limits {
    /// Local day of `nonfatal`.
    pub day: String,
    pub nonfatal: u32,
    /// Fingerprint (with the app version) to the time it was last reported.
    pub seen: BTreeMap<String, i64>,
}

impl Limits {
    /// Whether a non-fatal report with this fingerprint may be captured now; counts it if so.
    pub fn allow(&mut self, key: &str, today: &str, now: i64) -> bool {
        if self.day != today {
            self.day = today.to_string();
            self.nonfatal = 0;
        }
        self.seen.retain(|_, at| now - *at < SAME_ERROR_AGAIN_AFTER && *at <= now);
        if self.nonfatal >= MAX_NONFATAL_PER_DAY || self.seen.contains_key(key) {
            return false;
        }
        self.nonfatal += 1;
        self.seen.insert(key.to_string(), now);
        true
    }
}

struct State {
    dirs: Dirs,
    store: Store,
    /// Changed in memory (counters) and not saved yet.
    dirty: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Missing or corrupt file: defaults (a corrupt one is overwritten on the next save).
pub fn load(dirs: &Dirs) -> Store {
    std::fs::read(dirs.store()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(dirs: &Dirs, store: &Store) -> std::io::Result<()> {
    std::fs::create_dir_all(&dirs.config)?;
    let json = serde_json::to_vec_pretty(store).map_err(std::io::Error::other)?;
    crate::session::write_file_atomic(&dirs.store(), &json)
}

/// Loads telemetry.json, makes the crash id on first use and keeps both for this process.
pub fn init(dirs: Dirs) -> Store {
    let mut store = load(&dirs);
    let created = store.crash_id.is_none();
    if created {
        store.crash_id = Some(new_id());
    }
    // Saved before the crash reporter starts: it reads the same id.
    let saved = if created { save(&dirs, &store) } else { Ok(()) };
    *lock() = Some(State { dirs, store: store.clone(), dirty: false });
    if let Err(err) = saved {
        log::warn!("could not save telemetry.json: {err}");
    }
    store
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn dirs() -> Option<Dirs> {
    lock().as_ref().map(|s| s.dirs.clone())
}

pub fn get() -> Option<Store> {
    lock().as_ref().map(|s| s.store.clone())
}

/// Applies `change` and saves at once when anything changed (ids, flags, queued events).
pub fn update<T>(change: impl FnOnce(&mut Store) -> T) -> Option<T> {
    let (out, saved) = {
        let mut guard = lock();
        let state = guard.as_mut()?;
        let before = state.store.clone();
        let out = change(&mut state.store);
        let saved = if state.store != before || state.dirty {
            state.dirty = false;
            save(&state.dirs, &state.store)
        } else {
            Ok(())
        };
        (out, saved)
    };
    if let Err(err) = saved {
        log::warn!("could not save telemetry.json: {err}");
    }
    Some(out)
}

/// Applies `change` in memory only (counters, limits); `flush` saves it later.
pub fn update_quiet<T>(change: impl FnOnce(&mut Store) -> T) -> Option<T> {
    let mut guard = lock();
    let state = guard.as_mut()?;
    state.dirty = true;
    Some(change(&mut state.store))
}

/// Saves what `update_quiet` changed (every few minutes and on exit).
pub fn flush() {
    let saved = {
        let mut guard = lock();
        match guard.as_mut() {
            Some(state) if state.dirty => {
                state.dirty = false;
                save(&state.dirs, &state.store)
            }
            _ => Ok(()),
        }
    };
    if let Err(err) = saved {
        log::warn!("could not save telemetry.json: {err}");
    }
}

/// Appends an upload to sent.jsonl, keeping the last 50.
pub fn record_sent(item: &SentItem) {
    let Some(dirs) = dirs() else { return };
    let path = dirs.sent_log();
    let mut items = read_sent(&path);
    items.push(item.clone());
    let keep = items.len().saturating_sub(SENT_LOG_MAX);
    let mut text = String::new();
    for item in &items[keep..] {
        if let Ok(line) = serde_json::to_string(item) {
            text.push_str(&line);
            text.push('\n');
        }
    }
    let written =
        std::fs::create_dir_all(&dirs.data).and_then(|()| crate::session::write_file_atomic(&path, text.as_bytes()));
    if let Err(err) = written {
        log::warn!("could not write the sent log: {err}");
    }
}

fn read_sent(path: &std::path::Path) -> Vec<SentItem> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// "Show what was sent": newest first.
pub fn history() -> Vec<SentItem> {
    let Some(dirs) = dirs() else { return Vec::new() };
    let mut items = read_sent(&dirs.sent_log());
    items.reverse();
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrip_and_old_files() {
        let dir = std::env::temp_dir().join(format!("wellsnap-test-telemetry-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dirs = Dirs::under(&dir, &dir);
        assert_eq!(load(&dirs), Store::default());
        let store = Store { crash_id: Some("c".into()), consent_seen: true, dont_send_streak: 2, ..Default::default() };
        save(&dirs, &store).unwrap();
        assert_eq!(load(&dirs), store);
        let json = std::fs::read_to_string(dirs.store()).unwrap();
        assert!(json.contains("\"crashId\"") && json.contains("\"dontSendStreak\""));
        std::fs::write(dirs.store(), r#"{"usageId": "u"}"#).unwrap();
        assert_eq!(load(&dirs).usage_id.as_deref(), Some("u"), "missing fields default");
        std::fs::write(dirs.store(), "{ nope").unwrap();
        assert_eq!(load(&dirs), Store::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_error_once_a_day_and_at_most_30() {
        let mut limits = Limits::default();
        assert!(limits.allow("0.1.0|a", "2026-10-22", 1000));
        assert!(!limits.allow("0.1.0|a", "2026-10-22", 2000), "same fingerprint");
        assert!(limits.allow("0.1.1|a", "2026-10-22", 2000), "a new version reports it again");
        assert!(limits.allow("0.1.0|a", "2026-10-23", 1000 + SAME_ERROR_AGAIN_AFTER), "a day later");
        for i in 0..MAX_NONFATAL_PER_DAY {
            limits.allow(&format!("k{i}"), "2026-10-24", 200_000);
        }
        assert_eq!(limits.nonfatal, MAX_NONFATAL_PER_DAY);
        assert!(!limits.allow("new", "2026-10-24", 200_001), "daily cap");
        assert!(limits.allow("new", "2026-10-25", 200_002), "next day");
    }

    #[test]
    fn retry_backs_off_up_to_six_hours() {
        let mut retry = Retry::default();
        assert!(retry.ready(0));
        retry.back_off(100);
        assert_eq!((retry.not_before, retry.backoff_secs), (160, 60));
        assert!(!retry.ready(159) && retry.ready(160));
        retry.back_off(200);
        assert_eq!(retry.backoff_secs, 120);
        for _ in 0..20 {
            retry.back_off(0);
        }
        assert_eq!(retry.backoff_secs, BACKOFF_MAX);
        retry.succeeded();
        assert_eq!(retry, Retry::default());
        retry.wait(10, 30);
        assert_eq!(retry.not_before, 40);
    }
}
