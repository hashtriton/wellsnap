//! Saving screenshots to disk. Files are never overwritten: an existing name gets a numeric suffix, and the
//! file is opened with create_new so an old file can never be partially overwritten (the aCropalypse class
//! of leaks).

use chrono::{DateTime, Local};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// ASCII-only stem, safe for chats, archives and links: WellSnap_2026-10-22_14-05-33
pub fn timestamp_stem(now: DateTime<Local>) -> String {
    format!("WellSnap_{}", now.format("%Y-%m-%d_%H-%M-%S"))
}

/// Writes `bytes` to `dir/stem.ext`, or `stem_1.ext`, `stem_2.ext`... if taken. Returns the final path.
pub fn save_unique(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    for n in 0..10_000u32 {
        let name = if n == 0 { format!("{stem}.{ext}") } else { format!("{stem}_{n}.{ext}") };
        let path = dir.join(name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                let result = f.write_all(bytes).and_then(|_| f.sync_all());
                if let Err(err) = result {
                    drop(f);
                    let _ = fs::remove_file(&path);
                    return Err(err);
                }
                return Ok(path);
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::other("could not find a free file name"))
}

/// The configured folder, or the user's Desktop (Known Folder lookup, so OneDrive-redirected Desktops
/// work), or the home folder as a last resort.
pub fn resolve_save_dir(configured: Option<&str>, desktop: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = configured.map(str::trim).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    desktop.or(home).unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wellsnap-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn stem_is_ascii_and_sortable() {
        let t = Local.with_ymd_and_hms(2026, 10, 22, 14, 5, 33).unwrap();
        assert_eq!(timestamp_stem(t), "WellSnap_2026-10-22_14-05-33");
    }

    #[test]
    fn never_overwrites() {
        let dir = temp_dir("export-unique");
        let a = save_unique(&dir, "shot", "png", b"first").unwrap();
        let b = save_unique(&dir, "shot", "png", b"second").unwrap();
        let c = save_unique(&dir, "shot", "png", b"third").unwrap();
        assert!(a.ends_with("shot.png") && b.ends_with("shot_1.png") && c.ends_with("shot_2.png"));
        assert_eq!(fs::read(&a).unwrap(), b"first");
        assert_eq!(fs::read(&b).unwrap(), b"second");
    }

    #[test]
    fn save_dir_resolution() {
        let desk = Some(PathBuf::from("D:/Desk"));
        let home = Some(PathBuf::from("D:/Home"));
        assert_eq!(resolve_save_dir(Some(" E:/Shots "), desk.clone(), home.clone()), PathBuf::from("E:/Shots"));
        assert_eq!(resolve_save_dir(Some(""), desk.clone(), home.clone()), PathBuf::from("D:/Desk"));
        assert_eq!(resolve_save_dir(None, None, home), PathBuf::from("D:/Home"));
    }
}
