//! Persisted user settings: one JSON file in the app config dir, written atomically.

use crate::donations::DonationState;
use crate::i18n::LanguagePref;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpg,
}

/// Where mark comments go: drawn next to the numbers on the screenshot, or kept as separate text while the
/// image carries only the numbers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum CommentPlacement {
    #[default]
    OnImage,
    Separate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum EnterAction {
    #[default]
    Copy,
    Save,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: LanguagePref,
    pub theme: Theme,
    /// Accelerator in global-hotkey syntax, e.g. "PrintScreen" or "Ctrl+Shift+2".
    pub capture_hotkey: String,
    /// Registered only while a snapshot set exists.
    pub smart_paste_hotkey: String,
    pub autostart: bool,
    /// None means the user's Desktop.
    pub save_dir: Option<String>,
    pub image_format: ImageFormat,
    pub jpg_quality: u8,
    /// Remembered choice of the overlay toggle "text on the image / text separately".
    pub comment_placement: CommentPlacement,
    pub enter_action: EnterAction,
    pub show_hints: bool,
    /// Exclude the snapshot strip from screen capture and screen sharing.
    pub hide_strip_from_capture: bool,
    /// Per-tool colors and widths, owned by the overlay editor.
    pub tool_prefs: serde_json::Value,
    pub onboarding_done: bool,
    pub donations: DonationState,
    /// Background update check (shortly after start, then every 12 hours). The About button works regardless.
    pub auto_check_updates: bool,
    /// A found update installs by itself while the computer is idle (see updates/policy.rs). Needs the check on.
    pub auto_install_updates: bool,
    /// The user approved turning off the Windows "Print Screen opens Snipping Tool" setting. Kept here, not only
    /// in the registry, because a reinstall without /UPDATE restores the setting and clears the registry marker.
    pub print_screen_taken_over: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: LanguagePref::Auto,
            theme: Theme::System,
            capture_hotkey: default_capture_hotkey().to_string(),
            smart_paste_hotkey: default_smart_paste_hotkey().to_string(),
            autostart: true,
            save_dir: None,
            image_format: ImageFormat::Png,
            jpg_quality: 90,
            comment_placement: CommentPlacement::OnImage,
            enter_action: EnterAction::Copy,
            show_hints: true,
            hide_strip_from_capture: true,
            tool_prefs: serde_json::Value::Object(Default::default()),
            onboarding_done: false,
            donations: DonationState::default(),
            auto_check_updates: true,
            auto_install_updates: true,
            print_screen_taken_over: false,
        }
    }
}

pub fn default_capture_hotkey() -> &'static str {
    if cfg!(target_os = "macos") { "Cmd+Shift+2" } else { "PrintScreen" }
}

/// Windows: Win+Alt+V, which no Windows shortcut uses. The old default Ctrl+Alt+V is Paste Special in Office.
pub fn default_smart_paste_hotkey() -> &'static str {
    if cfg!(target_os = "macos") { "Ctrl+Cmd+V" } else { "Super+Alt+V" }
}

impl Settings {
    /// Clamp values that may have been edited by hand.
    pub fn sanitize(&mut self) {
        self.jpg_quality = self.jpg_quality.clamp(40, 100);
        if self.capture_hotkey.trim().is_empty() {
            self.capture_hotkey = default_capture_hotkey().to_string();
        }
        if self.smart_paste_hotkey.trim().is_empty() {
            self.smart_paste_hotkey = default_smart_paste_hotkey().to_string();
        }
        if self.save_dir.as_deref().is_some_and(|dir| dir.trim().is_empty()) {
            self.save_dir = None;
        }
        if !self.tool_prefs.is_object() {
            self.tool_prefs = serde_json::Value::Object(Default::default());
        }
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Missing file gives defaults; a corrupt file is kept aside as settings.json.bad and replaced by defaults.
pub fn load(path: &Path) -> Settings {
    let mut settings = match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
            Ok(s) => s,
            Err(err) => {
                log::warn!("settings file is corrupt ({err}), using defaults");
                let _ = fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    };
    settings.sanitize();
    settings
}

pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wellsnap-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_and_defaults() {
        let dir = temp_dir("settings-roundtrip");
        let path = settings_path(&dir);
        assert_eq!(load(&path), Settings::default());
        let s = Settings {
            capture_hotkey: "Ctrl+Shift+X".into(),
            donations: DonationState { captures: 7, ..Default::default() },
            ..Default::default()
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
    }

    #[test]
    fn partial_file_fills_defaults() {
        let dir = temp_dir("settings-partial");
        let path = settings_path(&dir);
        fs::write(&path, r#"{"commentPlacement": "separate", "jpgQuality": 5}"#).unwrap();
        let s = load(&path);
        assert_eq!(s.comment_placement, CommentPlacement::Separate);
        assert_eq!(s.jpg_quality, 40, "quality is clamped");
        assert_eq!(s.capture_hotkey, default_capture_hotkey());
    }

    #[test]
    fn new_fields_default_for_old_files() {
        let dir = temp_dir("settings-new-fields");
        let path = settings_path(&dir);
        fs::write(&path, r#"{"onboardingDone": true}"#).unwrap();
        let s = load(&path);
        assert!(s.auto_check_updates);
        assert!(s.auto_install_updates);
        assert!(!s.print_screen_taken_over);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["autoCheckUpdates"], true);
        assert_eq!(json["autoInstallUpdates"], true);
        assert_eq!(json["printScreenTakenOver"], false);
    }

    #[test]
    fn default_hotkeys_are_valid_and_a_chosen_one_is_kept() {
        assert!(crate::hotkeys::validate(default_capture_hotkey()).is_ok());
        assert!(crate::hotkeys::validate(default_smart_paste_hotkey()).is_ok());
        let mut s = Settings { smart_paste_hotkey: "Ctrl+Alt+V".into(), ..Default::default() };
        s.sanitize();
        assert_eq!(s.smart_paste_hotkey, "Ctrl+Alt+V", "the user may pick the old default on purpose");
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = temp_dir("settings-corrupt");
        let path = settings_path(&dir);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), Settings::default());
        assert!(path.with_extension("json.bad").exists());
    }
}
