//! Which other program holds the capture hotkey (Windows): known screenshot tools found among the running
//! processes of this session, matched against their default keys, plus alternative keys WellSnap could use.
//! Names are localized here, so the toast and the webviews show the same ones. macOS keeps its generic texts.
//!
//! Contract only: the process scan, the contender table and the startup toast arrive with WP2.
#![allow(dead_code)] // Until WP2 adds the bodies and the caller in hotkeys.rs.

use crate::state::HotkeyErrorKind;
use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ContenderId {
    OneDrive,
    Dropbox,
    Lightshot,
    YandexDisk,
    ShareX,
    Greenshot,
    Flameshot,
    Snagit,
    PicPick,
    Gyazo,
    Screenpresso,
    FastStone,
    Joxi,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContenderRef {
    pub id: ContenderId,
    /// Display name in the UI language.
    pub name: String,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    /// Known contenders running in this session.
    pub running: Vec<ContenderRef>,
    /// Running contenders whose defaults include the capture key, most likely first (only when it is taken).
    pub suspects: Vec<ContenderRef>,
    /// Running contenders that may react to the same key although WellSnap registered it.
    pub may_double: Vec<ContenderRef>,
    /// Free keys to offer instead, in accelerator syntax.
    pub alternatives: Vec<String>,
}

/// Kind of a registration error, from the text global-hotkey and WellSnap produce.
pub fn classify(_err: &str) -> HotkeyErrorKind {
    HotkeyErrorKind::Other
}

/// Scans the running programs; empty on macOS. Blocking: not on the main thread.
pub fn report(_app: &AppHandle) -> ConflictReport {
    ConflictReport::default()
}

/// After the first hotkey registration at startup: warns once when another program took the capture key.
/// Spawns its own thread; does nothing on macOS. It starts before `lib.rs` re-applies a lost Print Screen
/// take-over on another thread, so Snipping may still hold the key then. Check `printscreen::status()` in the
/// same thread: while Snipping is on it is the excluded Snipping case and no warning is shown.
pub fn start_startup_check(_app: &AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The string members of `export type ContenderId = ...;` in ipc.ts.
    fn ipc_contender_ids() -> Vec<String> {
        let ipc = include_str!("../../src/shared/ipc.ts");
        let start = ipc.find("export type ContenderId =").expect("ContenderId in ipc.ts");
        let union = &ipc[start..start + ipc[start..].find(';').unwrap()];
        union.split('"').skip(1).step_by(2).map(String::from).collect()
    }

    #[test]
    fn contender_ids_match_ipc() {
        use ContenderId::*;
        let all = [
            OneDrive, Dropbox, Lightshot, YandexDisk, ShareX, Greenshot, Flameshot, Snagit, PicPick, Gyazo,
            Screenpresso, FastStone, Joxi,
        ];
        let sent: Vec<String> =
            all.iter().map(|id| serde_json::to_value(id).unwrap().as_str().unwrap().to_owned()).collect();
        assert_eq!(sent, ipc_contender_ids());
    }

    #[test]
    fn report_shape_matches_ipc() {
        let sharex = ContenderRef { id: ContenderId::ShareX, name: "ShareX".into() };
        let yandex = ContenderRef { id: ContenderId::YandexDisk, name: "Yandex Disk".into() };
        let report = ConflictReport {
            running: vec![sharex.clone(), yandex.clone()],
            suspects: vec![sharex],
            may_double: vec![yandex],
            alternatives: vec!["Shift+PrintScreen".into()],
        };
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({
                "running": [{"id": "shareX", "name": "ShareX"}, {"id": "yandexDisk", "name": "Yandex Disk"}],
                "suspects": [{"id": "shareX", "name": "ShareX"}],
                "mayDouble": [{"id": "yandexDisk", "name": "Yandex Disk"}],
                "alternatives": ["Shift+PrintScreen"],
            })
        );
    }
}
