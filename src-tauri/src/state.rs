//! Shared application state, managed by Tauri (`app.state::<AppState>()`).

use crate::i18n::Lang;
use crate::overlay::OverlayState;
use crate::session::Session;
use crate::settings::{self, Settings};
use crate::updates::UpdatePhase;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

/// Why a hotkey is not registered, classified from the error text (conflicts::classify).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyErrorKind {
    /// Another program holds the key.
    Taken,
    /// A clipboard or undo chord WellSnap refuses to take.
    Reserved,
    Other,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub capture_registered: bool,
    /// Human-readable reason when registration failed (e.g. taken by another app or by Snipping Tool).
    pub capture_error: Option<String>,
    pub capture_error_kind: Option<HotkeyErrorKind>,
    pub smart_paste_registered: bool,
    pub smart_paste_error: Option<String>,
    pub smart_paste_error_kind: Option<HotkeyErrorKind>,
}

/// The last single screenshot copied with separate comment text: smart paste can then paste the text and the
/// picture as one message even without a snapshot set.
pub struct LastCopy {
    pub png: Vec<u8>,
    pub text: String,
}

pub struct AppState {
    pub settings_path: PathBuf,
    /// Root for snapshot-set folders (files that go to the clipboard as a file list).
    pub paste_root: PathBuf,
    pub settings: Mutex<Settings>,
    pub session: Mutex<Option<Session>>,
    /// With its generation, so an expiry timer only clears the copy it was started for.
    pub last_copy: Mutex<Option<(u64, LastCopy)>>,
    pub overlay: Mutex<OverlayState>,
    pub hotkeys: Mutex<HotkeyStatus>,
    /// Settings records a new combination: nothing may be registered until it resumes or closes.
    pub hotkeys_suspended: AtomicBool,
    /// Newer version found by the last update check, and whether it is being installed.
    pub update: Mutex<UpdatePhase>,
    /// One-picture sheets composed by the strip webview.
    pub sheets: crate::sheet::SheetJobs,
}

impl AppState {
    pub fn new(settings_path: PathBuf, paste_root: PathBuf) -> Self {
        let settings = settings::load(&settings_path);
        Self {
            settings_path,
            paste_root,
            settings: Mutex::new(settings),
            session: Mutex::new(None),
            last_copy: Mutex::new(None),
            overlay: Mutex::new(OverlayState::default()),
            hotkeys: Mutex::new(HotkeyStatus::default()),
            hotkeys_suspended: AtomicBool::new(false),
            update: Mutex::new(UpdatePhase::Idle),
            sheets: crate::sheet::SheetJobs::default(),
        }
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(self.settings.lock().unwrap().language)
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Smart paste has something to paste: a snapshot set, or a single copy with separate text.
    pub fn has_smart_paste_content(&self) -> bool {
        self.session.lock().unwrap().as_ref().is_some_and(|s| !s.is_empty()) || self.last_copy.lock().unwrap().is_some()
    }

    /// Applies `change`, persists the result and returns the new settings.
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Settings {
        let mut guard = self.settings.lock().unwrap();
        change(&mut guard);
        guard.sanitize();
        if let Err(err) = settings::save(&self.settings_path, &guard) {
            log::error!("could not save settings: {err}");
        }
        guard.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_status_sends_the_error_kinds() {
        let status = HotkeyStatus {
            capture_error: Some("HotKey already registered: PrintScreen".into()),
            capture_error_kind: Some(HotkeyErrorKind::Taken),
            smart_paste_error_kind: Some(HotkeyErrorKind::Reserved),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({
                "captureRegistered": false,
                "captureError": "HotKey already registered: PrintScreen",
                "captureErrorKind": "taken",
                "smartPasteRegistered": false,
                "smartPasteError": null,
                "smartPasteErrorKind": "reserved",
            })
        );
        assert_eq!(serde_json::to_value(HotkeyErrorKind::Other).unwrap(), "other");
    }
}
