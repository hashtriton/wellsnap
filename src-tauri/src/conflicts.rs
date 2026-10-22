//! Which other program holds the capture hotkey (Windows): known screenshot tools found among the running
//! processes of this session, matched against their default keys, plus alternative keys WellSnap could use.
//! Names are localized here, so the toast and the webviews show the same ones. macOS keeps its generic texts:
//! there every registration failure reads the same, so nothing is scanned.
//!
//! Windows has no API that names the owner of a hotkey, and hook-based tools never make the registration fail,
//! so the report only says who is *likely*: a running contender whose default keys include the capture key.
//! WellSnap never closes or reconfigures another program; the fixes are its own hotkey and Windows settings.

use crate::state::HotkeyErrorKind;
use serde::Serialize;
use tauri::AppHandle;
#[cfg(any(windows, test))]
use {crate::i18n::Lang, crate::state::HotkeyStatus, std::str::FromStr, tauri_plugin_global_shortcut::Shortcut};

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))] // Only the Windows scan finds contenders.
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
#[cfg_attr(not(windows), allow(dead_code))]
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

/// How a contender takes its keys; the order is how likely it is to be the one that made registration fail.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mechanism {
    /// Exclusive: whoever registers first wins, the other gets "already registered".
    RegisterHotKey,
    Unknown,
    /// Keyboard hook or a non-exclusive registration: both programs react, registration says nothing.
    Hook,
}

#[cfg(any(windows, test))]
struct Contender {
    id: ContenderId,
    /// (Russian, English).
    name: (&'static str, &'static str),
    /// Process names, lowercase, without ".exe".
    procs: &'static [&'static str],
    mechanism: Mechanism,
    /// Keys it takes out of the box, in accelerator syntax.
    defaults: &'static [&'static str],
}

#[cfg(any(windows, test))]
impl Contender {
    fn name(&self, lang: Lang) -> &'static str {
        if lang == Lang::Ru { self.name.0 } else { self.name.1 }
    }

    fn binds(&self, accelerator: &str) -> bool {
        self.defaults.iter().any(|key| same_key(key, accelerator))
    }
}

/// Windows screenshot tools that take Print Screen combinations by default (research: features-f2.md).
/// Process names marked unverified there: yandexdisk2, yandexdisk, gyazo, joxi.
#[cfg(any(windows, test))]
const CONTENDERS: &[Contender] = {
    use ContenderId::*;
    use Mechanism::*;
    &[
        Contender {
            id: OneDrive,
            name: ("OneDrive", "OneDrive"),
            procs: &["onedrive"],
            mechanism: RegisterHotKey,
            defaults: &["PrintScreen", "Alt+PrintScreen"],
        },
        Contender {
            id: Dropbox,
            name: ("Dropbox", "Dropbox"),
            procs: &["dropbox"],
            mechanism: Unknown,
            defaults: &["PrintScreen", "Ctrl+PrintScreen"],
        },
        Contender {
            id: Lightshot,
            name: ("Lightshot", "Lightshot"),
            procs: &["lightshot"],
            mechanism: Unknown,
            defaults: &["PrintScreen"],
        },
        Contender {
            id: YandexDisk,
            name: ("Яндекс Диск", "Yandex Disk"),
            procs: &["yandexdisk2", "yandexdisk"],
            mechanism: Hook,
            defaults: &["PrintScreen", "Alt+PrintScreen"],
        },
        Contender {
            id: ShareX,
            name: ("ShareX", "ShareX"),
            procs: &["sharex"],
            mechanism: RegisterHotKey,
            defaults: &[
                "PrintScreen",
                "Ctrl+PrintScreen",
                "Alt+PrintScreen",
                "Shift+PrintScreen",
                "Ctrl+Shift+PrintScreen",
            ],
        },
        Contender {
            id: Greenshot,
            name: ("Greenshot", "Greenshot"),
            procs: &["greenshot"],
            mechanism: RegisterHotKey,
            defaults: &["PrintScreen", "Alt+PrintScreen", "Ctrl+PrintScreen", "Shift+PrintScreen"],
        },
        Contender {
            id: Flameshot,
            name: ("Flameshot", "Flameshot"),
            procs: &["flameshot"],
            mechanism: RegisterHotKey,
            defaults: &["PrintScreen", "Shift+PrintScreen"],
        },
        Contender {
            id: Snagit,
            name: ("Snagit", "Snagit"),
            procs: &["snagit32", "snagit64", "snagitcapture"],
            mechanism: Hook,
            defaults: &["PrintScreen"],
        },
        Contender {
            id: PicPick,
            name: ("PicPick", "PicPick"),
            procs: &["picpick"],
            mechanism: RegisterHotKey,
            defaults: &[
                "PrintScreen",
                "Alt+PrintScreen",
                "Ctrl+PrintScreen",
                "Shift+PrintScreen",
                "Ctrl+Alt+PrintScreen",
                "Ctrl+Shift+PrintScreen",
                "Ctrl+Shift+Alt+PrintScreen",
            ],
        },
        Contender {
            id: Gyazo,
            name: ("Gyazo", "Gyazo"),
            procs: &["gyazo"],
            mechanism: Hook,
            defaults: &["PrintScreen"],
        },
        Contender {
            id: Screenpresso,
            name: ("Screenpresso", "Screenpresso"),
            procs: &["screenpresso"],
            mechanism: Unknown,
            defaults: &["PrintScreen"],
        },
        Contender {
            id: FastStone,
            name: ("FastStone Capture", "FastStone Capture"),
            procs: &["fscapture"],
            mechanism: Unknown,
            defaults: &["PrintScreen", "Alt+PrintScreen"],
        },
        Contender {
            id: Joxi,
            name: ("Joxi", "Joxi"),
            procs: &["joxi"],
            mechanism: Unknown,
            defaults: &["Ctrl+PrintScreen"],
        },
    ]
};

/// Keys offered instead of a taken one, in this order: they keep the Print Screen habit.
#[cfg(any(windows, test))]
const ALTERNATIVES: &[&str] = &["Ctrl+PrintScreen", "Shift+PrintScreen", "Ctrl+Alt+PrintScreen"];

#[cfg(any(windows, test))]
const PRINT_SCREEN: &str = "PrintScreen";

/// Kind of a registration error, from the text global-hotkey and WellSnap produce. On Windows global-hotkey
/// reports ERROR_HOTKEY_ALREADY_REGISTERED as "HotKey already registered: ..."; on macOS every failure reads
/// "Unable to register hotkey: ...", so a taken key cannot be told apart there.
pub fn classify(err: &str) -> HotkeyErrorKind {
    if err == crate::hotkeys::RESERVED {
        HotkeyErrorKind::Reserved
    } else if err.starts_with("HotKey already registered") {
        HotkeyErrorKind::Taken
    } else {
        HotkeyErrorKind::Other
    }
}

/// Scans the running programs; empty on macOS. Blocking: not on the main thread.
#[cfg(windows)]
pub fn report(app: &AppHandle) -> ConflictReport {
    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let settings = state.settings();
    let status = state.hotkeys.lock().unwrap().clone();
    let lang = state.lang();
    let running = match_running(&platform::running_contender_names());
    let outcome = Outcome::of(&status);
    let (suspects, may_double) =
        verdict(&running, &settings.capture_hotkey, outcome, || crate::printscreen::status().onedrive_enabled);
    let alternatives = match outcome {
        Outcome::Failed(HotkeyErrorKind::Taken) => {
            alternatives(&running, &settings.capture_hotkey, &settings.smart_paste_hotkey)
        }
        _ => Vec::new(),
    };
    let refs = |ids: &[ContenderId]| -> Vec<ContenderRef> { ids.iter().map(|&id| contender_ref(id, lang)).collect() };
    ConflictReport { running: refs(&running), suspects: refs(&suspects), may_double: refs(&may_double), alternatives }
}

#[cfg(not(windows))]
pub fn report(_app: &AppHandle) -> ConflictReport {
    ConflictReport::default()
}

/// After the first hotkey registration at startup: warns once when another program took the capture key.
/// Spawns its own thread; does nothing on macOS. It starts before `lib.rs` re-applies a lost Print Screen
/// take-over on another thread, so Snipping may still hold the key then. `printscreen::status()` is checked in
/// the same thread: while Snipping is on it is the excluded Snipping case and no warning is shown.
#[cfg(windows)]
pub fn start_startup_check(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || startup_check(&app));
}

#[cfg(not(windows))]
pub fn start_startup_check(_app: &AppHandle) {}

#[cfg(windows)]
fn startup_check(app: &AppHandle) {
    use crate::i18n::tr;
    use tauri::Manager;
    /// Longer than the 1 s WM_SETTINGCHANGE broadcast of a take-over that `lib.rs` may be applying right now.
    const TAKE_OVER_SETTLE: std::time::Duration = std::time::Duration::from_millis(1500);

    let state = app.state::<crate::state::AppState>();
    let mut status = crate::hotkeys::settled_status(app);
    let settings = state.settings();
    if !may_warn(status.capture_error_kind, settings.onboarding_done) {
        return;
    }
    let accelerator = settings.capture_hotkey.clone();
    let print_screen = crate::printscreen::status();
    if snipping_holds(&print_screen, &accelerator) {
        return;
    }
    // The re-applied take-over (lib.rs) may have switched Snipping off just now, after the first registration
    // failed because of it: register once more before blaming another program.
    if settings.print_screen_taken_over && print_screen.changed_by_us && same_key(&accelerator, PRINT_SCREEN) {
        std::thread::sleep(TAKE_OVER_SETTLE);
        crate::hotkeys::apply(app);
        status = crate::hotkeys::settled_status(app);
        if !may_warn(status.capture_error_kind, settings.onboarding_done) {
            return;
        }
    }

    let running = match_running(&platform::running_contender_names());
    let (suspects, _) = verdict(&running, &accelerator, Outcome::Failed(HotkeyErrorKind::Taken), || {
        print_screen.onedrive_enabled
    });
    let first = suspects.first().copied();
    let fingerprint = fingerprint(&accelerator, first);
    let last = settings.hotkey_warning.as_deref();
    if !should_warn(status.capture_error_kind, settings.onboarding_done, last, &fingerprint) {
        return;
    }
    log::warn!("capture hotkey {accelerator} is taken; running: {running:?}, suspects: {suspects:?}");
    let lang = state.lang();
    let title = tr(lang, "toast.hotkeyOff").replace("{hotkey}", &crate::tray::display_hotkey(&accelerator));
    let detail = match first {
        Some(id) => tr(lang, "toast.hotkeyOffBy").replace("{app}", contender(id).name(lang)),
        None => tr(lang, "toast.hotkeyOffUnknown").to_string(),
    };
    crate::notice::toast_error(app, &title, Some(&detail));
    // Only for the key it was about: the user may have picked another one meanwhile.
    state.update_settings(|s| {
        if s.capture_hotkey == accelerator {
            s.hotkey_warning = Some(fingerprint);
        }
    });
}

/// What the last registration of the capture key did.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Registered,
    Failed(HotkeyErrorKind),
    /// Not registered and no error: the hotkeys are suspended while Settings records a key, or not applied yet.
    Unknown,
}

#[cfg(any(windows, test))]
impl Outcome {
    fn of(status: &HotkeyStatus) -> Outcome {
        match (status.capture_registered, status.capture_error_kind) {
            (true, _) => Outcome::Registered,
            (false, Some(kind)) => Outcome::Failed(kind),
            (false, None) => Outcome::Unknown,
        }
    }
}

/// Accelerators compared as shortcuts, so "ctrl+printscreen" equals "Ctrl+PrintScreen".
#[cfg(any(windows, test))]
fn same_key(a: &str, b: &str) -> bool {
    matches!((Shortcut::from_str(a), Shortcut::from_str(b)), (Ok(a), Ok(b)) if a == b)
}

#[cfg(any(windows, test))]
fn contender(id: ContenderId) -> &'static Contender {
    CONTENDERS.iter().find(|c| c.id == id).expect("every contender id is in the table")
}

#[cfg(windows)]
fn contender_ref(id: ContenderId, lang: Lang) -> ContenderRef {
    ContenderRef { id, name: contender(id).name(lang).to_string() }
}

/// The contender a process belongs to, from its exe name or path, ignoring case and ".exe".
#[cfg(any(windows, test))]
fn contender_for(exe: &str) -> Option<ContenderId> {
    let file = exe.trim().rsplit(['\\', '/']).next().unwrap_or_default().to_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    CONTENDERS.iter().find(|c| c.procs.contains(&stem)).map(|c| c.id)
}

/// Contenders among the running processes, in table order, each once.
#[cfg(any(windows, test))]
fn match_running(exe_names: &[String]) -> Vec<ContenderId> {
    let found: Vec<ContenderId> = exe_names.iter().filter_map(|exe| contender_for(exe)).collect();
    CONTENDERS.iter().map(|c| c.id).filter(|id| found.contains(id)).collect()
}

/// (suspects, may_double) for the capture key. Suspects only when the key is taken: running contenders that
/// bind it, the exclusive kind first. When none binds it but OneDrive's screenshot setting is on (it may run
/// under another exe name or start later), OneDrive is named for the keys it takes. may_double only when
/// WellSnap registered the key: running contenders that may still react to it (hook or unknown kind).
#[cfg(any(windows, test))]
fn verdict(
    running: &[ContenderId],
    accelerator: &str,
    outcome: Outcome,
    onedrive_blocking: impl FnOnce() -> bool,
) -> (Vec<ContenderId>, Vec<ContenderId>) {
    let mut binding: Vec<&Contender> =
        running.iter().map(|&id| contender(id)).filter(|c| c.binds(accelerator)).collect();
    match outcome {
        Outcome::Failed(HotkeyErrorKind::Taken) => {
            binding.sort_by_key(|c| c.mechanism);
            let mut suspects: Vec<ContenderId> = binding.iter().map(|c| c.id).collect();
            if suspects.is_empty() && contender(ContenderId::OneDrive).binds(accelerator) && onedrive_blocking() {
                suspects.push(ContenderId::OneDrive);
            }
            (suspects, Vec::new())
        }
        Outcome::Registered => {
            let shared = binding.iter().filter(|c| c.mechanism != Mechanism::RegisterHotKey);
            (Vec::new(), shared.map(|c| c.id).collect())
        }
        Outcome::Failed(_) | Outcome::Unknown => (Vec::new(), Vec::new()),
    }
}

/// Candidates that no running contender takes by default and that are not WellSnap's own keys.
#[cfg(any(windows, test))]
fn alternatives(running: &[ContenderId], capture: &str, smart_paste: &str) -> Vec<String> {
    ALTERNATIVES
        .iter()
        .filter(|key| !same_key(key, capture) && !same_key(key, smart_paste))
        .filter(|key| !running.iter().any(|&id| contender(id).binds(key)))
        .map(|key| key.to_string())
        .collect()
}

/// Snipping Tool holds Print Screen: the existing Print Screen card explains it, so no conflict warning.
#[cfg(any(windows, test))]
fn snipping_holds(print_screen: &crate::printscreen::PrintScreenStatus, accelerator: &str) -> bool {
    print_screen.applicable && print_screen.snipping_enabled && same_key(accelerator, PRINT_SCREEN)
}

/// The startup warning is about a taken key only, and waits until onboarding, which shows the problem itself,
/// is done.
#[cfg(any(windows, test))]
fn may_warn(kind: Option<HotkeyErrorKind>, onboarding_done: bool) -> bool {
    kind == Some(HotkeyErrorKind::Taken) && onboarding_done
}

/// Once per fingerprint (stored in `Settings::hotkey_warning`).
#[cfg(any(windows, test))]
fn should_warn(kind: Option<HotkeyErrorKind>, onboarding_done: bool, last: Option<&str>, fingerprint: &str) -> bool {
    may_warn(kind, onboarding_done) && last != Some(fingerprint)
}

/// "{accelerator}|taken|{first suspect id or none}": a new owner of the same key warns again.
#[cfg(any(windows, test))]
fn fingerprint(accelerator: &str, first_suspect: Option<ContenderId>) -> String {
    let owner = first_suspect
        .and_then(|id| serde_json::to_value(id).ok())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "none".to_string());
    format!("{accelerator}|taken|{owner}")
}

#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::GetCurrentProcessId;

    /// Exe names of the running processes that are known contenders, in this user's session only: hotkeys are
    /// per session, and on a terminal server the snapshot lists every user's processes. The snapshot gives names
    /// without opening processes, so elevated ones are seen too.
    pub fn running_contender_names() -> Vec<String> {
        let Some(own) = session_of(unsafe { GetCurrentProcessId() }) else { return Vec::new() };
        let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log::warn!("process snapshot failed: {err}");
                return Vec::new();
            }
        };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut names = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            // The session lookup only for the few names that matter.
            if super::contender_for(&name).is_some() && session_of(entry.th32ProcessID) == Some(own) {
                names.push(name);
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
        }
        let _ = unsafe { CloseHandle(snapshot) };
        names
    }

    /// None when the process is gone or not accessible (then it is not counted).
    fn session_of(pid: u32) -> Option<u32> {
        let mut session = 0;
        unsafe { ProcessIdToSessionId(pid, &mut session) }.ok().map(|_| session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ContenderId::*;

    const ALL: [ContenderId; 13] = [
        OneDrive, Dropbox, Lightshot, YandexDisk, ShareX, Greenshot, Flameshot, Snagit, PicPick, Gyazo, Screenpresso,
        FastStone, Joxi,
    ];

    /// The string members of `export type ContenderId = ...;` in ipc.ts.
    fn ipc_contender_ids() -> Vec<String> {
        let ipc = include_str!("../../src/shared/ipc.ts");
        let start = ipc.find("export type ContenderId =").expect("ContenderId in ipc.ts");
        let union = &ipc[start..start + ipc[start..].find(';').unwrap()];
        union.split('"').skip(1).step_by(2).map(String::from).collect()
    }

    fn names(exes: &[&str]) -> Vec<String> {
        exes.iter().map(|s| s.to_string()).collect()
    }

    fn taken(running: &[ContenderId], accelerator: &str) -> Vec<ContenderId> {
        verdict(running, accelerator, Outcome::Failed(HotkeyErrorKind::Taken), || false).0
    }

    fn doubles(running: &[ContenderId], accelerator: &str) -> Vec<ContenderId> {
        verdict(running, accelerator, Outcome::Registered, || false).1
    }

    #[test]
    fn contender_ids_match_ipc() {
        let sent: Vec<String> =
            ALL.iter().map(|id| serde_json::to_value(id).unwrap().as_str().unwrap().to_owned()).collect();
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

    #[test]
    fn classify_reads_the_registration_errors() {
        // The text the plugin passes through from global-hotkey 0.8 (`#[error("HotKey already registered: {0:?}")]`).
        let shortcut = Shortcut::from_str("PrintScreen").unwrap();
        let taken =
            tauri_plugin_global_shortcut::Error::GlobalHotkey(format!("HotKey already registered: {shortcut:?}"));
        assert_eq!(classify(&taken.to_string()), HotkeyErrorKind::Taken);
        assert_eq!(classify(crate::hotkeys::RESERVED), HotkeyErrorKind::Reserved);
        assert_eq!(
            classify("Unable to register hotkey: RegisterEventHotKey failed for Digit2"),
            HotkeyErrorKind::Other
        );
        assert_eq!(classify("Access is denied. (os error 5)"), HotkeyErrorKind::Other);
        assert_eq!(classify(""), HotkeyErrorKind::Other);
    }

    #[test]
    fn table_is_complete_and_valid() {
        assert_eq!(CONTENDERS.len(), ALL.len());
        for id in ALL {
            assert_eq!(CONTENDERS.iter().filter(|c| c.id == id).count(), 1, "{id:?} once");
        }
        for c in CONTENDERS {
            assert!(!c.name.0.is_empty() && !c.name.1.is_empty(), "{:?}", c.id);
            assert!(!c.procs.is_empty(), "{:?} has no process names", c.id);
            for proc in c.procs {
                assert_eq!(*proc, proc.to_lowercase(), "{proc}");
                assert!(!proc.ends_with(".exe"), "{proc}");
            }
            assert!(!c.defaults.is_empty(), "{:?} has no keys", c.id);
            for key in c.defaults {
                assert!(crate::hotkeys::validate(key).is_ok(), "{key} of {:?}", c.id);
            }
        }
        for key in ALTERNATIVES {
            assert!(crate::hotkeys::validate(key).is_ok(), "{key}");
        }
        let lang_names = |lang| CONTENDERS.iter().map(|c| c.name(lang)).collect::<Vec<_>>();
        assert!(lang_names(Lang::Ru).contains(&"Яндекс Диск"));
        assert!(lang_names(Lang::En).contains(&"Yandex Disk"));
    }

    #[test]
    fn running_programs_are_matched_by_exe_name() {
        assert_eq!(match_running(&names(&["OneDrive.exe"])), [OneDrive]);
        assert_eq!(match_running(&names(&["ONEDRIVE.EXE", "onedrive"])), [OneDrive], "each contender once");
        assert_eq!(match_running(&names(&["ShareX"])), [ShareX], "without .exe");
        assert_eq!(match_running(&names(&[r"C:\Program Files\ShareX\ShareX.exe"])), [ShareX], "a full path");
        let found = match_running(&names(&["Snagit64.exe", "YandexDisk2.exe", "FSCapture.exe"]));
        assert_eq!(found, [YandexDisk, Snagit, FastStone]);
        assert_eq!(match_running(&names(&["picpick.exe", "Dropbox.exe"])), [Dropbox, PicPick], "in table order");
        // Generic names never match.
        assert!(match_running(&names(&["browser.exe", "explorer.exe", "sharex-helper.exe", "", "exe"])).is_empty());
    }

    #[test]
    fn taken_key_names_the_programs_that_bind_it() {
        assert_eq!(taken(&[ShareX], "PrintScreen"), [ShareX]);
        assert_eq!(taken(&[ShareX], "printscreen"), [ShareX], "shortcut equality, not text");
        assert_eq!(taken(&[ShareX], "ctrl+printscreen"), [ShareX]);
        assert!(taken(&[Lightshot], "Ctrl+PrintScreen").is_empty(), "Lightshot takes only bare Print Screen");
        assert!(taken(&[], "PrintScreen").is_empty());
        // Exclusive registration first, then unknown, then hooks; running order (the table's) within a kind.
        let running = [Gyazo, Dropbox, OneDrive, Lightshot, ShareX];
        assert_eq!(taken(&running, "PrintScreen"), [OneDrive, ShareX, Dropbox, Lightshot, Gyazo]);
        assert_eq!(taken(&[Snagit, Screenpresso], "PrintScreen"), [Screenpresso, Snagit]);
        // A taken key is never a "may double" note.
        assert!(verdict(&[Gyazo], "PrintScreen", Outcome::Failed(HotkeyErrorKind::Taken), || false).1.is_empty());
    }

    #[test]
    fn onedrive_is_named_when_nothing_running_binds_the_key() {
        let check = |running: &[ContenderId], accelerator: &str, blocking: bool| {
            verdict(running, accelerator, Outcome::Failed(HotkeyErrorKind::Taken), || blocking).0
        };
        assert_eq!(check(&[], "PrintScreen", true), [OneDrive]);
        assert_eq!(check(&[], "Alt+PrintScreen", true), [OneDrive]);
        assert!(check(&[], "PrintScreen", false).is_empty());
        assert!(check(&[], "Ctrl+Shift+2", true).is_empty(), "OneDrive does not take this key");
        // A running program that binds the key comes first, without the OneDrive guess.
        assert_eq!(check(&[Lightshot], "PrintScreen", true), [Lightshot]);
        // The setting is read only when needed.
        let suspects = verdict(&[ShareX], "PrintScreen", Outcome::Failed(HotkeyErrorKind::Taken), || {
            panic!("not needed")
        });
        assert_eq!(suspects.0, [ShareX]);
    }

    /// Lists this machine's contenders and times the scan. Run with `cargo test -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn scan_this_machine() {
        let started = std::time::Instant::now();
        let names = platform::running_contender_names();
        println!("{names:?} in {:?}", started.elapsed());
    }

    #[test]
    fn registered_key_notes_programs_that_may_also_react() {
        assert_eq!(doubles(&[Gyazo], "PrintScreen"), [Gyazo]);
        assert_eq!(doubles(&[Dropbox], "PrintScreen"), [Dropbox], "unknown kind");
        assert!(doubles(&[ShareX, OneDrive], "PrintScreen").is_empty(), "exclusive kinds cannot share the key");
        assert!(doubles(&[Gyazo], "Ctrl+PrintScreen").is_empty());
        let running = [Lightshot, YandexDisk, Greenshot, Snagit];
        assert_eq!(doubles(&running, "PrintScreen"), [Lightshot, YandexDisk, Snagit]);
        // Registered: nobody is blamed.
        assert!(verdict(&[Dropbox, ShareX], "PrintScreen", Outcome::Registered, || true).0.is_empty());
    }

    #[test]
    fn other_outcomes_name_no_one() {
        for outcome in [
            Outcome::Failed(HotkeyErrorKind::Reserved),
            Outcome::Failed(HotkeyErrorKind::Other),
            Outcome::Unknown,
        ] {
            assert_eq!(verdict(&[ShareX, Gyazo], "PrintScreen", outcome, || true), (vec![], vec![]), "{outcome:?}");
        }
    }

    #[test]
    fn outcome_from_the_hotkey_status() {
        let status = |registered, kind| HotkeyStatus {
            capture_registered: registered,
            capture_error_kind: kind,
            ..Default::default()
        };
        assert_eq!(Outcome::of(&status(true, None)), Outcome::Registered);
        let taken = Some(HotkeyErrorKind::Taken);
        assert_eq!(Outcome::of(&status(false, taken)), Outcome::Failed(HotkeyErrorKind::Taken));
        // Suspended while Settings records a key.
        assert_eq!(Outcome::of(&HotkeyStatus::default()), Outcome::Unknown);
    }

    #[test]
    fn alternatives_skip_keys_running_programs_take() {
        let all = ["Ctrl+PrintScreen", "Shift+PrintScreen", "Ctrl+Alt+PrintScreen"];
        assert_eq!(alternatives(&[], "PrintScreen", "Ctrl+Alt+V"), all);
        assert_eq!(alternatives(&[Lightshot], "PrintScreen", "Ctrl+Alt+V"), all);
        // ShareX takes Ctrl+ and Shift+PrintScreen.
        assert_eq!(alternatives(&[ShareX], "PrintScreen", "Ctrl+Alt+V"), ["Ctrl+Alt+PrintScreen"]);
        let without_ctrl = ["Shift+PrintScreen", "Ctrl+Alt+PrintScreen"];
        assert_eq!(alternatives(&[Dropbox], "PrintScreen", "Ctrl+Alt+V"), without_ctrl);
        // PicPick takes all three: nothing to offer.
        assert!(alternatives(&[PicPick], "PrintScreen", "Ctrl+Alt+V").is_empty());
        // Never WellSnap's own keys.
        assert_eq!(alternatives(&[], "ctrl+printscreen", "Shift+PrintScreen"), ["Ctrl+Alt+PrintScreen"]);
    }

    #[test]
    fn snipping_case_is_left_to_the_print_screen_card() {
        use crate::printscreen::PrintScreenStatus;
        let on = PrintScreenStatus { applicable: true, snipping_enabled: true, ..Default::default() };
        assert!(snipping_holds(&on, "PrintScreen"));
        assert!(snipping_holds(&on, "printscreen"));
        assert!(!snipping_holds(&on, "Ctrl+PrintScreen"));
        assert!(!snipping_holds(&PrintScreenStatus { snipping_enabled: false, ..on.clone() }, "PrintScreen"));
        assert!(!snipping_holds(&PrintScreenStatus { applicable: false, ..on }, "PrintScreen"));
    }

    #[test]
    fn startup_warning_once_per_fingerprint_after_onboarding() {
        let taken = Some(HotkeyErrorKind::Taken);
        let fp = fingerprint("PrintScreen", Some(OneDrive));
        assert_eq!(fp, "PrintScreen|taken|oneDrive");
        assert_eq!(fingerprint("Ctrl+PrintScreen", None), "Ctrl+PrintScreen|taken|none");
        assert_eq!(fingerprint("PrintScreen", Some(YandexDisk)), "PrintScreen|taken|yandexDisk");

        assert!(should_warn(taken, true, None, &fp));
        assert!(!should_warn(taken, true, Some(&fp), &fp), "already shown");
        assert!(should_warn(taken, true, Some("PrintScreen|taken|none"), &fp), "a new owner warns again");
        assert!(!should_warn(taken, false, None, &fp), "not before onboarding is done");
        for kind in [Some(HotkeyErrorKind::Reserved), Some(HotkeyErrorKind::Other), None] {
            assert!(!should_warn(kind, true, None, &fp), "{kind:?}");
        }
    }
}
