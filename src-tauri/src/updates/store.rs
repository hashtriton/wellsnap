//! update-state.json in the app config dir: the updater's own bookkeeping. Kept apart from settings.json, so
//! the settings broadcasts and the core-owned settings fields stay as they are.

use super::policy::{self, StartupOutcome};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateStore {
    /// Version of the previous run; tells an update apart from a first run.
    pub last_run_version: Option<String>,
    pub last_check_at: Option<i64>,
    /// When the offered version was first seen: a release that holds itself back counts from here.
    pub first_seen: Option<FirstSeen>,
    /// Written right before an install; a start that is still the old version counts it as failed.
    pub attempt: Option<Attempt>,
    /// A version whose package failed signature verification: never installed automatically.
    pub rejected: Option<String>,
    pub last_update: Option<LastUpdate>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FirstSeen {
    pub version: String,
    pub at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attempt {
    pub to: String,
    /// Attempts made for `to`; all but a successful one have failed.
    pub count: u32,
    pub at: i64,
    /// A snapshot set existed and is lost with the restart.
    pub set_cleared: bool,
    /// Release notes copied before the install: afterwards latest.json no longer reports anything newer.
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LastUpdate {
    /// None when the previous version kept no marker.
    pub from: Option<String>,
    pub to: String,
    pub notes: Option<String>,
    pub at: i64,
    pub set_cleared: bool,
    /// The "updated" notice still has to be shown.
    pub notice_pending: bool,
    /// The user hid "What's new" in About.
    pub notes_hidden: bool,
}

impl UpdateStore {
    /// Applies what this start means and records the running version.
    pub fn record_startup(&mut self, outcome: &StartupOutcome, current: &str, now: i64) {
        let not_newer = |v: &str| policy::compare_versions(v, current).is_some_and(|o| o.is_le());
        match outcome {
            StartupOutcome::Updated { from } => {
                let attempt = self.attempt.take().filter(|a| a.to == current);
                self.last_update = Some(LastUpdate {
                    from: from.clone(),
                    to: current.to_string(),
                    notes: attempt.as_ref().and_then(|a| a.notes.clone()),
                    at: now,
                    set_cleared: attempt.is_some_and(|a| a.set_cleared),
                    notice_pending: true,
                    notes_hidden: false,
                });
            }
            StartupOutcome::InstallFailed { to } => {
                log::warn!("the update to {to} did not complete; still running {current}");
            }
            StartupOutcome::FirstRun | StartupOutcome::Same | StartupOutcome::Older => {
                if self.attempt.as_ref().is_some_and(|a| not_newer(&a.to)) {
                    self.attempt = None;
                }
            }
        }
        if self.first_seen.as_ref().is_some_and(|f| not_newer(&f.version)) {
            self.first_seen = None;
        }
        if self.rejected.as_deref().is_some_and(not_newer) {
            self.rejected = None;
        }
        if let Some(last) = &mut self.last_update
            && last.notice_pending
            && now - last.at > policy::NOTICE_GIVE_UP.as_secs() as i64
        {
            last.notice_pending = false;
        }
        self.last_run_version = Some(current.to_string());
    }

    /// Remembers when `version` was first offered; returns that time.
    pub fn note_offered(&mut self, version: &str, now: i64) -> i64 {
        match &self.first_seen {
            Some(f) if f.version == version => f.at,
            _ => {
                self.first_seen = Some(FirstSeen { version: version.to_string(), at: now });
                now
            }
        }
    }

    /// Failed attempts for `version` (the attempt in progress included until the next start clears it).
    pub fn attempts_for(&self, version: &str) -> Option<&Attempt> {
        self.attempt.as_ref().filter(|a| a.to == version)
    }
}

static STORE: Mutex<Option<(PathBuf, UpdateStore)>> = Mutex::new(None);

/// Missing or corrupt file: defaults (a corrupt one is logged and overwritten on the next save).
pub fn load(path: &Path) -> UpdateStore {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            log::warn!("update state is corrupt ({err}), starting over");
            UpdateStore::default()
        }),
        Err(_) => UpdateStore::default(),
    }
}

pub fn save(path: &Path, store: &UpdateStore) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(store).map_err(std::io::Error::other)?;
    crate::session::write_file_atomic(path, &json)
}

pub fn init(path: PathBuf) -> UpdateStore {
    let store = load(&path);
    *STORE.lock().unwrap() = Some((path, store.clone()));
    store
}

pub fn get() -> UpdateStore {
    STORE.lock().unwrap().as_ref().map(|(_, s)| s.clone()).unwrap_or_default()
}

/// Applies `change` and saves when it changed anything. Returns the new state.
pub fn update<T>(change: impl FnOnce(&mut UpdateStore) -> T) -> (UpdateStore, T) {
    let mut guard = STORE.lock().unwrap();
    let Some((path, store)) = guard.as_mut() else {
        let mut scratch = UpdateStore::default();
        let out = change(&mut scratch);
        return (scratch, out);
    };
    let before = store.clone();
    let out = change(store);
    if *store != before
        && let Err(err) = save(path, store)
    {
        log::error!("could not save the update state: {err}");
    }
    (store.clone(), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wellsnap-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("update-state.json")
    }

    #[test]
    fn roundtrip_missing_and_corrupt() {
        let path = temp_path("update-store");
        assert_eq!(load(&path), UpdateStore::default());
        let store = UpdateStore {
            last_run_version: Some("0.1.0".into()),
            attempt: Some(Attempt { to: "0.2.0".into(), count: 1, at: 5, set_cleared: true, notes: Some("n".into()) }),
            ..Default::default()
        };
        save(&path, &store).unwrap();
        assert_eq!(load(&path), store);
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"lastRunVersion\"") && json.contains("\"setCleared\""));
        std::fs::write(&path, "{ nope").unwrap();
        assert_eq!(load(&path), UpdateStore::default());
        // Written before 0.3.0, with the rollout bucket that is gone now.
        std::fs::write(&path, r#"{"bucket": 3, "lastCheckAt": 9}"#).unwrap();
        assert_eq!(load(&path).last_check_at, Some(9), "missing fields default, old ones are ignored");
    }

    #[test]
    fn a_successful_update_moves_the_attempt_into_last_update() {
        let mut s = UpdateStore {
            last_run_version: Some("0.1.0".into()),
            first_seen: Some(FirstSeen { version: "0.2.0".into(), at: 1 }),
            attempt: Some(Attempt { to: "0.2.0".into(), count: 1, at: 2, set_cleared: true, notes: Some("notes".into()) }),
            ..Default::default()
        };
        s.record_startup(&StartupOutcome::Updated { from: Some("0.1.0".into()) }, "0.2.0", 100);
        let last = s.last_update.clone().unwrap();
        assert_eq!(last.from.as_deref(), Some("0.1.0"));
        assert_eq!(last.notes.as_deref(), Some("notes"));
        assert!(last.set_cleared && last.notice_pending && !last.notes_hidden);
        assert_eq!(last.at, 100);
        assert_eq!(s.attempt, None);
        assert_eq!(s.first_seen, None, "the installed version no longer waits");
        assert_eq!(s.last_run_version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn a_manual_update_without_attempt_has_no_notes() {
        let mut s = UpdateStore::default();
        s.record_startup(&StartupOutcome::Updated { from: None }, "0.2.0", 100);
        let last = s.last_update.unwrap();
        assert_eq!((last.from, last.notes, last.set_cleared), (None, None, false));
    }

    #[test]
    fn an_update_by_hand_drops_an_attempt_at_a_newer_version() {
        let attempt = Attempt { to: "0.3.0".into(), count: 1, at: 2, notes: Some("0.3.0 notes".into()), ..Default::default() };
        let mut s = UpdateStore { last_run_version: Some("0.1.0".into()), attempt: Some(attempt), ..Default::default() };
        s.record_startup(&StartupOutcome::Updated { from: Some("0.1.0".into()) }, "0.2.0", 100);
        assert_eq!(s.attempt, None);
        let last = s.last_update.unwrap();
        assert_eq!((last.to.as_str(), last.notes), ("0.2.0", None), "not the notes of 0.3.0");
    }

    #[test]
    fn a_failed_install_keeps_the_attempt() {
        let attempt = Attempt { to: "0.2.0".into(), count: 2, at: 2, ..Default::default() };
        let mut s = UpdateStore { last_run_version: Some("0.1.0".into()), attempt: Some(attempt.clone()), ..Default::default() };
        s.record_startup(&StartupOutcome::InstallFailed { to: "0.2.0".into() }, "0.1.0", 100);
        assert_eq!(s.attempt, Some(attempt));
        assert_eq!(s.last_update, None);
        assert_eq!(s.attempts_for("0.2.0").map(|a| a.count), Some(2));
        assert_eq!(s.attempts_for("0.3.0"), None);
    }

    #[test]
    fn the_notice_is_given_up_after_a_week() {
        let week = policy::NOTICE_GIVE_UP.as_secs() as i64;
        let pending = LastUpdate { to: "0.2.0".into(), at: 0, notice_pending: true, ..Default::default() };
        let mut s = UpdateStore { last_run_version: Some("0.2.0".into()), last_update: Some(pending), ..Default::default() };
        s.record_startup(&StartupOutcome::Same, "0.2.0", week);
        assert!(s.last_update.as_ref().unwrap().notice_pending);
        s.record_startup(&StartupOutcome::Same, "0.2.0", week + 1);
        assert!(!s.last_update.as_ref().unwrap().notice_pending);
    }

    #[test]
    fn first_seen_is_kept_per_version() {
        let mut s = UpdateStore::default();
        assert_eq!(s.note_offered("0.2.0", 10), 10);
        assert_eq!(s.note_offered("0.2.0", 99), 10, "same version: same start");
        assert_eq!(s.note_offered("0.3.0", 50), 50, "a newer version waits from its own start");
    }
}
