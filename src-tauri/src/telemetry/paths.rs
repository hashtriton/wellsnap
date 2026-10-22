//! Where telemetry keeps its files. Resolved before Tauri starts (the crash reporter process never gets an
//! AppHandle), from the same folders Tauri's path resolver uses: `start` compares them and logs a mismatch.
//! Debug builds use their own names, like update-state.dev.json, so a dev run never touches the installed
//! app's ids, counters or outbox.

use std::path::{Path, PathBuf};

/// `identifier` in tauri.conf.json (checked by a test).
pub const IDENTIFIER: &str = "one.welldone.wellsnap";

#[derive(Debug, Clone, PartialEq)]
pub struct Dirs {
    /// `app_config_dir`: settings.json and telemetry.json.
    pub config: PathBuf,
    /// `app_local_data_dir()/telemetry`: outbox, minidumps and the sent log; removed by the uninstaller.
    pub data: PathBuf,
}

impl Dirs {
    pub fn detect() -> Option<Dirs> {
        let (config, local) = base_dirs()?;
        Some(Self::under(&config, &local))
    }

    pub fn under(config_base: &Path, local_base: &Path) -> Dirs {
        let folder = if cfg!(debug_assertions) { "telemetry-dev" } else { "telemetry" };
        Dirs { config: config_base.join(IDENTIFIER), data: local_base.join(IDENTIFIER).join(folder) }
    }

    pub fn settings(&self) -> PathBuf {
        crate::settings::settings_path(&self.config)
    }

    pub fn store(&self) -> PathBuf {
        self.config.join(if cfg!(debug_assertions) { "telemetry.dev.json" } else { "telemetry.json" })
    }

    pub fn outbox(&self) -> PathBuf {
        self.data.join("outbox")
    }

    pub fn dumps(&self) -> PathBuf {
        self.data.join("dumps")
    }

    pub fn sent_log(&self) -> PathBuf {
        self.data.join("sent.jsonl")
    }
}

/// Roaming and local application data on Windows (FOLDERID_RoamingAppData and FOLDERID_LocalAppData, which
/// Windows also exports as these variables); Application Support for both on macOS.
fn base_dirs() -> Option<(PathBuf, PathBuf)> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    #[cfg(windows)]
    return Some((var("APPDATA")?, var("LOCALAPPDATA")?));
    #[cfg(target_os = "macos")]
    return var("HOME").map(|home| home.join("Library").join("Application Support")).map(|dir| (dir.clone(), dir));
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = var;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_matches_the_tauri_config() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], IDENTIFIER);
    }

    #[test]
    fn files_live_under_the_app_folders() {
        let dirs = Dirs::under(Path::new("R"), Path::new("L"));
        assert_eq!(dirs.config, Path::new("R").join(IDENTIFIER));
        assert_eq!(dirs.settings(), Path::new("R").join(IDENTIFIER).join("settings.json"));
        assert!(dirs.outbox().starts_with(Path::new("L").join(IDENTIFIER)));
        // A debug build must never share files with an installed release.
        let dev = cfg!(debug_assertions);
        assert_eq!(dirs.store().ends_with("telemetry.dev.json"), dev);
        assert_eq!(dirs.data.ends_with("telemetry-dev"), dev);
        assert_eq!(dirs.data.ends_with("telemetry"), !dev);
    }
}
