//! What happens after the user finishes a snapshot, and the snapshot-set actions (copy all, save all,
//! smart paste). Platform specifics live in clipboard/, paste.rs and export.rs.

use crate::clipboard;
use crate::export;
use crate::i18n::tr;
use crate::session::{self, Mark, Session};
use crate::state::{AppState, LastCopy};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

/// Telegram limits: 10 media per album, 1024 caption characters (4096 with Premium, unknown to us).
const TELEGRAM_ALBUM_MAX: usize = 10;
const TELEGRAM_CAPTION_MAX: usize = 1024;
/// Time for the target app to read the text before the clipboard is replaced with the files. Busy targets
/// (Telegram decoding an album preview, a cold Electron renderer) read the clipboard late, so this is generous.
const SMART_PASTE_GAP: Duration = Duration::from_millis(750);
/// How long smart paste waits for the user to release the hotkey's modifiers before giving up.
const MODIFIERS_RELEASE_TIMEOUT: Duration = Duration::from_millis(1500);
/// A single copy stays available to smart paste for this long; after that the hotkey is released again.
const LAST_COPY_TTL: Duration = Duration::from_secs(15 * 60);
/// Files of a cleared set or a removed snapshot stay this long: a paste in progress may still upload them.
const REMOVED_FILES_GRACE: Duration = Duration::from_secs(10 * 60);
const CACHE_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

static LAST_COPY_GENERATION: AtomicU64 = AtomicU64::new(0);
/// A smart paste is running; further presses (key repeat, our own synthetic keys) are dropped.
static SMART_PASTE_BUSY: AtomicBool = AtomicBool::new(false);
/// A donation reminder is scheduled or on screen and has not reported its outcome yet.
static NUDGE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FinishAction {
    /// Copy this snapshot only (bitmap + comments text).
    Copy,
    /// Save this snapshot to the save folder.
    Save,
    /// Add to the set and close; the next capture continues the set.
    AddToSet,
    /// Add to the set and copy the whole set as files.
    CopyAll,
}

/// Why the overlay finished the way it did, when the toast should say more than usual.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FinishHint {
    /// Lightshot's upload key (Ctrl+D): the copy happens, and the toast explains that nothing is uploaded.
    NoUpload,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishMeta {
    pub capture_id: u64,
    pub action: FinishAction,
    pub width: u32,
    pub height: u32,
    /// Letter drawn into the badges, if any (set actions always have one).
    pub letter: Option<char>,
    #[serde(default)]
    pub marks: Vec<Mark>,
    /// Comments are drawn on the image, so they are not repeated as text.
    #[serde(default)]
    pub comments_on_image: bool,
    /// "png" or "jpg": the encoding of the body.
    pub format: String,
    /// Physical pixels per CSS pixel of the capture monitor (1.0 from an overlay that does not send it).
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default)]
    #[allow(dead_code)] // Read by the noUpload toast (WP1).
    pub hint: Option<FinishHint>,
    /// Annotations per drawing tool, for the usage counters. Keys come from the webview: only the names in
    /// the overlay's DRAW_TOOLS may become counters.
    #[serde(default)]
    #[allow(dead_code)] // Read by the usage counters (telemetry package).
    pub tools: BTreeMap<String, u32>,
}

fn one() -> f32 {
    1.0
}

/// What a successful finish reports once the overlay is gone.
enum Finished {
    /// With text, the copy is kept for smart paste.
    Copied(Option<LastCopy>),
    Saved(PathBuf),
    Added,
    /// Added to the set; the copy of the whole set may still have failed.
    AddedAndCopied(Result<(), String>),
}

/// Runs the action while the overlay stays open, so a failed copy, save or add keeps the annotated snapshot:
/// the overlay is hidden only on success. On failure the overlay shows its own per-action headline and the
/// returned reason under it.
pub fn finish(app: &AppHandle, label: &str, meta: FinishMeta, image: Vec<u8>) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut ov = state.overlay.lock().unwrap();
        // `finishing` turns away a second call for the same capture while the first one is running.
        if !ov.active || ov.capture_id != meta.capture_id || ov.finishing {
            return Err("stale capture".into());
        }
        ov.finishing = true;
        ov.last_monitor = ov.monitors.iter().find(|m| crate::overlay::label_for(m) == label).cloned();
    }
    let lang = state.lang();
    let marks = session::clean_marks(meta.marks.clone());

    let result = match meta.action {
        FinishAction::Copy => {
            let text = if meta.comments_on_image { String::new() } else { single_text(meta.letter, &marks) };
            copy_single(image, text).map(Finished::Copied)
        }
        FinishAction::Save => save_one(app, &image, &meta.format).map(Finished::Saved),
        FinishAction::AddToSet | FinishAction::CopyAll => {
            add_to_set(app, &image, &meta, marks.clone()).map(|()| match meta.action {
                FinishAction::CopyAll => Finished::AddedAndCopied(copy_set_to_clipboard(app)),
                _ => Finished::Added,
            })
        }
    };
    let finished = match result {
        Ok(finished) => finished,
        Err(err) => {
            log::error!("{:?} failed: {err}", meta.action);
            let mut ov = state.overlay.lock().unwrap();
            if ov.capture_id == meta.capture_id {
                ov.finishing = false;
            }
            return Err(err);
        }
    };

    // Cancel ignores a running finish, but a destroyed overlay still ends the capture; then a new capture may
    // already be on screen and must not be hidden or have its letter taken.
    let still_ours = {
        let ov = state.overlay.lock().unwrap();
        ov.active && ov.capture_id == meta.capture_id
    };
    // A single copy or save with lettered badges keeps its letter: the next snapshot added to the set gets the
    // following one, so a chat never shows two different screenshots labelled C1. Done before the hide, while
    // no new capture can read the next letter.
    let reserved = still_ours
        && matches!(meta.action, FinishAction::Copy | FinishAction::Save)
        && !marks.is_empty()
        && reserve_letter(app, meta.letter);
    if still_ours {
        crate::overlay::hide_all(app);
    } else {
        log::warn!("capture {} ended while {:?} ran", meta.capture_id, meta.action);
    }
    if reserved {
        after_set_changed(app);
    }
    let mut nudge = true;
    match finished {
        Finished::Copied(copy) => {
            let with_text = copy.is_some();
            set_last_copy(app, copy);
            if with_text {
                let hotkey = crate::tray::display_hotkey(&state.settings().smart_paste_hotkey);
                // Promise the hotkey only when it is really registered (it may be taken by another app).
                let key = if crate::hotkeys::settled_status(app).smart_paste_registered {
                    "toast.smartPasteHint"
                } else {
                    "toast.smartPasteOff"
                };
                let detail = tr(lang, key).replace("{hotkey}", &hotkey);
                crate::notice::toast_with_detail(app, tr(lang, "toast.copied"), &detail);
            } else {
                crate::notice::toast(app, tr(lang, "toast.copied"));
            }
        }
        Finished::Saved(path) => {
            set_last_copy(app, None);
            crate::notice::toast_with_detail(app, tr(lang, "toast.saved"), &path.to_string_lossy());
        }
        Finished::Added => {
            nudge = false;
            set_last_copy(app, None);
            after_set_changed(app);
        }
        Finished::AddedAndCopied(copied) => {
            set_last_copy(app, None);
            after_set_changed(app);
            match copied {
                Ok(()) => crate::notice::toast(app, tr(lang, "toast.copiedAll")),
                // The snapshot is in the set already: retrying from the overlay would add it twice.
                Err(err) => {
                    nudge = false;
                    crate::notice::toast_error(app, tr(lang, "toast.copyFailed"), Some(&err));
                }
            }
        }
    }
    if nudge {
        record_and_maybe_nudge(app, marks.len() as u32);
    }
    Ok(())
}

/// Bitmap plus the comments as text. With text, the copy is returned for smart paste (Telegram drops clipboard
/// text that comes with a picture).
fn copy_single(png: Vec<u8>, text: String) -> Result<Option<LastCopy>, String> {
    let with_text = !text.is_empty();
    clipboard::copy_image(png.clone(), with_text.then(|| text.clone())).map_err(|e| e.to_string())?;
    Ok(with_text.then_some(LastCopy { png, text }))
}

/// Replaces the single copy available to smart paste and re-registers hotkeys when availability changes.
fn set_last_copy(app: &AppHandle, copy: Option<LastCopy>) {
    let state = app.state::<AppState>();
    let set = copy.is_some();
    let (had, generation) = {
        let mut slot = state.last_copy.lock().unwrap();
        // Taken under the lock, so generations follow the order in which copies are stored.
        let generation = LAST_COPY_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let had = slot.is_some();
        *slot = copy.map(|c| (generation, c));
        (had, generation)
    };
    if had != set {
        crate::hotkeys::apply(app);
    }
    if set {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(LAST_COPY_TTL);
            let state = app.state::<AppState>();
            let expired = {
                let mut slot = state.last_copy.lock().unwrap();
                let current = slot.as_ref().is_some_and(|(g, _)| *g == generation);
                if current {
                    slot.take();
                }
                current
            };
            if expired {
                crate::hotkeys::apply(&app);
            }
        });
    }
}

fn single_text(letter: Option<char>, marks: &[Mark]) -> String {
    match letter {
        None => session::single_text_block(marks),
        Some(l) => marks
            .iter()
            .filter(|m| !m.comment.is_empty())
            .map(|m| format!("{l}{} {}", m.num, m.comment))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn save_one(app: &AppHandle, image: &[u8], format: &str) -> Result<PathBuf, String> {
    let dir = save_dir(app);
    let ext = if format == "jpg" { "jpg" } else { "png" };
    export::save_unique(&dir, &export::timestamp_stem(chrono::Local::now()), ext, image).map_err(|e| e.to_string())
}

pub fn save_dir(app: &AppHandle) -> std::path::PathBuf {
    let state = app.state::<AppState>();
    let configured = state.settings().save_dir;
    export::resolve_save_dir(configured.as_deref(), app.path().desktop_dir().ok(), app.path().home_dir().ok())
}

/// The overlay's devicePixelRatio; anything that cannot be one falls back to 1.
fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

/// Stores the snapshot with the cleaned `marks`; the caller runs `after_set_changed` once the overlay is hidden.
fn add_to_set(app: &AppHandle, png: &[u8], meta: &FinishMeta, marks: Vec<Mark>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let letter = meta.letter;
    let added = {
        let mut guard = state.session.lock().unwrap();
        let session = guard.get_or_insert_with(|| Session::new(&state.paste_root, SystemTime::now()));
        if session.next_letter().is_none() {
            return Err(tr(state.lang(), "toast.sessionFull").to_string());
        }
        if letter.is_some() && letter != session.next_letter() {
            log::warn!("overlay drew letter {letter:?}, set expects {:?}", session.next_letter());
        }
        session
            .add(png, meta.width, meta.height, sane_scale(meta.scale), marks, meta.comments_on_image)
            .map_err(|e| e.to_string())?
    };
    log::info!("snapshot {added} added to the set");
    Ok(())
}

fn reserve_letter(app: &AppHandle, letter: Option<char>) -> bool {
    let Some(letter) = letter else { return false };
    let state = app.state::<AppState>();
    let mut guard = state.session.lock().unwrap();
    guard.as_mut().is_some_and(|s| s.skip_letter(letter))
}

pub fn after_set_changed(app: &AppHandle) {
    crate::windows_ui::sync_strip(app);
    crate::hotkeys::apply(app);
    crate::tray::refresh(app);
}

/// Strip "Copy all": the set becomes what smart paste pastes, even if a single copy came after it.
pub fn copy_all(app: &AppHandle) -> Result<(), String> {
    copy_set_to_clipboard(app)?;
    set_last_copy(app, None);
    crate::notice::toast(app, tr(app.state::<AppState>().lang(), "toast.copiedAll"));
    Ok(())
}

fn copy_set_to_clipboard(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let (files, text, session) = {
        let guard = state.session.lock().unwrap();
        let session = guard.as_ref().filter(|s| !s.is_empty()).ok_or("the set is empty")?;
        (session.files(), session.text_block(lang), session.clone())
    };
    // The HTML reads the files, so it is built outside the session lock.
    let html = session.html_block(lang);
    clipboard::copy_files(files, Some(text), Some(html)).map_err(|e| e.to_string())
}

pub fn save_all(app: &AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let dir = save_dir(app);
    let stem = export::timestamp_stem(chrono::Local::now());
    let snapshots = state.session.lock().unwrap().as_ref().map(|s| s.snapshots.clone()).unwrap_or_default();
    for snap in &snapshots {
        let bytes = std::fs::read(&snap.path).map_err(|e| e.to_string())?;
        export::save_unique(&dir, &format!("{stem}_{}", snap.letter), "png", &bytes).map_err(|e| e.to_string())?;
    }
    if !snapshots.is_empty() {
        crate::notice::toast_with_detail(app, tr(state.lang(), "toast.saved"), &dir.to_string_lossy());
        record_and_maybe_nudge(app, 0);
    }
    Ok(snapshots.len())
}

pub fn clear_set(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cleared = state.session.lock().unwrap().take();
    if let Some(session) = cleared {
        log::info!("set {} cleared", session.id);
        delete_later(app, session.dir);
    }
    after_set_changed(app);
}

pub fn remove_from_set(app: &AppHandle, letter: char) {
    let state = app.state::<AppState>();
    let removed = {
        let mut guard = state.session.lock().unwrap();
        let removed = guard.as_mut().and_then(|s| s.remove(letter));
        // Decided under the same lock, so a snapshot added meanwhile is never dropped with the session.
        if guard.as_ref().is_some_and(|s| s.is_empty()) {
            guard.take();
        }
        removed
    };
    if let Some(path) = removed {
        delete_later(app, path);
    }
    after_set_changed(app);
}

/// Deletes a removed snapshot file or a cleared set folder after the grace period, unless it belongs to the
/// current set by then.
fn delete_later(app: &AppHandle, path: PathBuf) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(REMOVED_FILES_GRACE);
        let in_use = app
            .state::<AppState>()
            .session
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.dir == path || s.snapshots.iter().any(|snap| snap.path == path));

        if in_use {
            return;
        }
        let result = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(err) = result
            && err.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("could not delete {}: {err}", path.display());
        }
    });
}

/// Removes old set folders while the app runs, not only at startup: a tray app may run for weeks.
pub fn start_cache_cleanup(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(CACHE_CLEANUP_INTERVAL);
            let state = app.state::<AppState>();
            let keep = state.session.lock().unwrap().as_ref().map(|s| s.id.clone());
            session::cleanup_old(&state.paste_root, keep.as_deref(), session::MAX_FOLDER_AGE);
        }
    });
}

/// What smart paste puts into the target besides the text.
enum PasteContent {
    /// A snapshot set: files A.png..Z.png (album / attachments).
    Files(Vec<PathBuf>),
    /// A single screenshot as a bitmap.
    Image(Vec<u8>),
}

impl PasteContent {
    fn count(&self) -> usize {
        match self {
            PasteContent::Files(files) => files.len(),
            PasteContent::Image(_) => 1,
        }
    }

    /// Without text and HTML: the text was pasted in the first step, and targets that read both (Word, or
    /// Chromium with a bitmap) would show it twice.
    fn write(self) -> Result<(), String> {
        match self {
            PasteContent::Files(files) => clipboard::copy_files(files, None, None),
            PasteContent::Image(png) => clipboard::copy_image(png, None),
        }
        .map_err(|e| e.to_string())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PastePlan {
    /// No text: one ordinary paste.
    ContentOnly,
    /// Paste the text into the field, then the pictures: one message (in Telegram the text becomes the caption).
    TextThenContent,
    /// Telegram cannot take this text as a caption (too long, or more than one album): paste the pictures and
    /// leave the text in the clipboard for a separate message.
    ContentThenTextInClipboard,
}

fn plan(text: &str, items: usize, telegram: bool) -> PastePlan {
    if text.is_empty() {
        PastePlan::ContentOnly
    } else if telegram && (text.chars().count() > TELEGRAM_CAPTION_MAX || items > TELEGRAM_ALBUM_MAX) {
        PastePlan::ContentThenTextInClipboard
    } else {
        PastePlan::TextThenContent
    }
}

/// Clears a busy flag when dropped, including on an early error return or a panic.
pub struct BusyGuard(&'static AtomicBool);

impl BusyGuard {
    pub fn claim(flag: &'static AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Self(flag))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Pastes the comments text and the pictures into the focused app as one message where the target supports
/// it. Uses the most recent result: a single copy with separate text if it came last, otherwise the set.
pub fn smart_paste(app: &AppHandle) {
    // One at a time: a second press, key repeat or our own synthetic Ctrl+V re-firing the hotkey is dropped.
    let Some(busy) = BusyGuard::claim(&SMART_PASTE_BUSY) else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        let _busy = busy;
        if let Err(err) = smart_paste_blocking(&app) {
            log::error!("smart paste failed: {err}");
            let state = app.state::<AppState>();
            crate::notice::toast_error(&app, tr(state.lang(), "toast.pasteFailed"), Some(&err));
        }
    });
}

fn smart_paste_blocking(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    // A single copy is cleared by every later finish and by "Copy all", so when it exists it is the newest.
    let last =
        state.last_copy.lock().unwrap().as_ref().map(|(_, c)| (PasteContent::Image(c.png.clone()), c.text.clone()));
    let (content, text) = match last {
        Some(found) => found,
        None => {
            let guard = state.session.lock().unwrap();
            let session = guard.as_ref().filter(|s| !s.is_empty()).ok_or("nothing to paste")?;
            (PasteContent::Files(session.files()), session.text_block(lang))
        }
    };
    if !crate::paste::can_send_input() {
        crate::paste::request_input_permission();
        return Err("input permission is missing".into());
    }
    // A synthetic Ctrl+V combined with held hotkey keys would be another chord (or this hotkey again).
    if !crate::paste::wait_modifiers_released(MODIFIERS_RELEASE_TIMEOUT) {
        return Err("release the hotkey keys and try again".into());
    }
    // Input to a window of higher integrity is dropped without an error; leave the clipboard untouched.
    if crate::paste::foreground_is_elevated() {
        return Err("the target window runs as administrator; WellSnap cannot paste into it".into());
    }
    let target = crate::paste::foreground_id();
    let telegram = crate::paste::foreground_app().is_some_and(|a| crate::paste::is_telegram(&a));

    match plan(&text, content.count(), telegram) {
        PastePlan::ContentOnly => {
            content.write()?;
            crate::paste::send_paste()
        }
        PastePlan::ContentThenTextInClipboard => {
            content.write()?;
            crate::paste::send_paste()?;
            std::thread::sleep(SMART_PASTE_GAP);
            clipboard::copy_text(text, false).map_err(|e| e.to_string())?;
            crate::notice::toast(app, tr(lang, "toast.textInClipboard"));
            Ok(())
        }
        PastePlan::TextThenContent => {
            clipboard::copy_text(text, true).map_err(|e| e.to_string())?;
            crate::paste::send_paste()?;
            std::thread::sleep(SMART_PASTE_GAP);
            content.write()?;
            // Focus moved during the pause (a notification click, Alt+Tab): the pictures would go elsewhere.
            if crate::paste::foreground_id() != target {
                crate::notice::toast(app, tr(lang, "toast.windowChanged"));
                return Ok(());
            }
            crate::paste::send_paste()
        }
    }
}

fn record_and_maybe_nudge(app: &AppHandle, marks: u32) {
    let state = app.state::<AppState>();
    let now = chrono::Utc::now().timestamp();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let settings = state.update_settings(|s| {
        s.donations.ensure_initialised(now);
        s.donations.record_capture(&today, marks);
    });
    if claim_nudge(&NUDGE_IN_FLIGHT, settings.donations.should_prompt(now)) {
        let index = settings.donations.prompts_shown;
        let captures = settings.donations.captures;
        let app = app.clone();
        std::thread::spawn(move || {
            // After the "Copied" toast, never on top of it.
            std::thread::sleep(Duration::from_millis(4200));
            let state = app.state::<AppState>();
            if state.overlay.lock().unwrap().active {
                nudge_settled();
            } else {
                crate::notice::nudge(&app, index, captures);
            }
        });
    }
}

/// Only one reminder at a time: a second finish within the delay must not schedule another card, which
/// would count one reminder episode twice.
fn claim_nudge(in_flight: &AtomicBool, should_prompt: bool) -> bool {
    should_prompt && in_flight.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
}

/// The reminder card reported its outcome (or was never shown); the next one may be scheduled.
pub fn nudge_settled() {
    NUDGE_IN_FLIGHT.store(false, Ordering::SeqCst);
}

pub fn nudge_in_flight() -> bool {
    NUDGE_IN_FLIGHT.load(Ordering::SeqCst)
}

pub fn smart_paste_busy() -> bool {
    SMART_PASTE_BUSY.load(Ordering::Acquire)
}

/// Held by the updater while it replaces the app, so a smart paste cannot start halfway.
pub fn claim_smart_paste() -> Option<BusyGuard> {
    BusyGuard::claim(&SMART_PASTE_BUSY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ImageFormat;

    #[test]
    fn single_text_uses_letter_when_drawn_with_one() {
        let marks = vec![Mark { num: 1, comment: "a".into() }, Mark { num: 2, comment: String::new() }];
        assert_eq!(single_text(None, &marks), "1 a");
        assert_eq!(single_text(Some('C'), &marks), "C1 a");
    }

    #[test]
    fn smart_paste_plans() {
        assert_eq!(plan("", 3, true), PastePlan::ContentOnly);
        assert_eq!(plan("A1 x", 3, false), PastePlan::TextThenContent);
        assert_eq!(plan("A1 x", 3, true), PastePlan::TextThenContent);
        assert_eq!(plan("A1 x", 11, true), PastePlan::ContentThenTextInClipboard);
        assert_eq!(plan("A1 x", 11, false), PastePlan::TextThenContent);
        let long = "я".repeat(TELEGRAM_CAPTION_MAX + 1);
        assert_eq!(plan(&long, 1, true), PastePlan::ContentThenTextInClipboard);
        assert_eq!(plan(&"я".repeat(TELEGRAM_CAPTION_MAX), 1, true), PastePlan::TextThenContent);
    }

    #[test]
    fn finish_meta_parses_from_frontend_json() {
        let meta: FinishMeta = serde_json::from_str(
            r#"{"captureId":3,"action":"copyAll","width":10,"height":20,"letter":"B","marks":[{"num":1,"comment":"x"}],"commentsOnImage":true,"format":"png"}"#,
        )
        .unwrap();
        assert_eq!(meta.action, FinishAction::CopyAll);
        assert_eq!(meta.letter, Some('B'));
        assert!(meta.comments_on_image);
        assert_eq!(meta.marks.len(), 1);
        assert_eq!(meta.scale, 1.0, "an overlay without the field");
        assert_eq!(meta.hint, None);
        assert!(meta.tools.is_empty());
    }

    #[test]
    fn finish_meta_carries_scale_hint_and_tools() {
        let meta: FinishMeta = serde_json::from_str(
            r#"{"captureId":4,"action":"copy","width":10,"height":20,"letter":null,"format":"png","scale":1.25,"hint":"noUpload","tools":{"arrow":2,"mark":1}}"#,
        )
        .unwrap();
        assert_eq!(meta.scale, 1.25);
        assert_eq!(meta.hint, Some(FinishHint::NoUpload));
        assert_eq!(meta.tools, BTreeMap::from([("arrow".to_string(), 2), ("mark".to_string(), 1)]));
        assert!(serde_json::from_str::<FinishMeta>(
            r#"{"captureId":4,"action":"copy","width":10,"height":20,"letter":null,"format":"png","hint":"upload"}"#
        )
        .is_err());
    }

    #[test]
    fn scale_from_the_overlay_is_sanitized() {
        assert_eq!(sane_scale(1.5), 1.5);
        assert_eq!(sane_scale(0.0), 1.0);
        assert_eq!(sane_scale(-2.0), 1.0);
        assert_eq!(sane_scale(f32::NAN), 1.0);
        assert_eq!(sane_scale(f32::INFINITY), 1.0);
    }

    #[test]
    fn image_format_is_known() {
        assert_eq!(serde_json::to_string(&ImageFormat::Jpg).unwrap(), "\"jpg\"");
    }

    #[test]
    fn only_one_nudge_is_scheduled_until_it_settles() {
        let flag = AtomicBool::new(false);
        assert!(!claim_nudge(&flag, false));
        assert!(claim_nudge(&flag, true));
        assert!(!claim_nudge(&flag, true), "a second finish within the delay");
        flag.store(false, Ordering::SeqCst);
        assert!(claim_nudge(&flag, true));
    }

    #[test]
    fn busy_guard_admits_one_and_clears_on_drop() {
        static FLAG: AtomicBool = AtomicBool::new(false);
        let first = BusyGuard::claim(&FLAG);
        assert!(first.is_some());
        assert!(BusyGuard::claim(&FLAG).is_none());
        drop(first);
        assert!(BusyGuard::claim(&FLAG).is_some());
        assert!(!FLAG.load(Ordering::SeqCst));
        let panicked = std::panic::catch_unwind(|| {
            let _guard = BusyGuard::claim(&FLAG).unwrap();
            panic!("paste failed");
        });
        assert!(panicked.is_err());
        assert!(!FLAG.load(Ordering::SeqCst), "cleared on panic");
    }
}
