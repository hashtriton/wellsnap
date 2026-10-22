//! What happens after the user finishes a snapshot, and the snapshot-set actions (copy all, save all,
//! smart paste). Platform specifics live in clipboard/, paste/ and export.rs.

use crate::clipboard;
use crate::export;
use crate::i18n::{Lang, tr};
use crate::paste::plan::{self, Class, Profile, Shape, Step};
use crate::paste::target::{self, Detected, Engine, Target};
use crate::session::{self, Mark, Session};
use crate::settings::SetCopyMode;
use crate::state::{AppState, LastCopy};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

/// Time for the target app to read the text before the clipboard is replaced with the files. Busy targets
/// (Telegram decoding an album preview, a cold Electron renderer) read the clipboard late, so this is generous.
const SMART_PASTE_GAP: Duration = Duration::from_millis(750);
/// Rows of the paste target registry that nobody has checked by hand yet (`Status::Planned`) paste like an
/// unknown program, in debug builds too. Set to true locally to try a planned profile.
const ALLOW_PLANNED: bool = false;
/// Sequential targets also stop when the foreground title changes (a chat or tab switched inside the same
/// window). Windows only; keep it while live tests show no false stops.
const TITLE_FINGERPRINT: bool = true;
/// How long smart paste waits for the user to release the hotkey's modifiers before giving up.
const MODIFIERS_RELEASE_TIMEOUT: Duration = Duration::from_millis(1500);
/// A single copy stays available to smart paste for this long; after that the hotkey is released again.
const LAST_COPY_TTL: Duration = Duration::from_secs(15 * 60);
/// Files of a cleared set or a removed snapshot stay this long: a paste in progress may still upload them.
const REMOVED_FILES_GRACE: Duration = Duration::from_secs(10 * 60);
const CACHE_CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// The paste shortcut named in the no-upload note.
const PASTE_KEYS: &str = if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" };

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
    /// Added to the set in one-picture mode: the picture is built and copied once the overlay is gone.
    AddedThenPicture,
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
                // The strip composes the picture, and for the first snapshot of a set it does not exist yet:
                // it appears with `after_set_changed`, after the hide.
                FinishAction::CopyAll if one_picture(app) => Finished::AddedThenPicture,
                FinishAction::CopyAll => Finished::AddedAndCopied(copy_set_to_clipboard(app)),
                _ => Finished::Added,
            })
        }
    };
    let finished = match result {
        Ok(finished) => finished,
        Err(err) => {
            log::error!("{:?} failed: {err}", meta.action);
            crate::telemetry::count("finish_failed");
            let mut ov = state.overlay.lock().unwrap();
            if ov.capture_id == meta.capture_id {
                ov.finishing = false;
            }
            return Err(err);
        }
    };
    crate::telemetry::finished(&meta, &marks);

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
            let smart_paste = with_text.then(|| {
                let hotkey = crate::tray::display_hotkey(&state.settings().smart_paste_hotkey);
                // Promise the hotkey only when it is really registered (it may be taken by another app).
                let key = if crate::hotkeys::settled_status(app).smart_paste_registered {
                    "toast.smartPasteHint"
                } else {
                    "toast.smartPasteOff"
                };
                tr(lang, key).replace("{hotkey}", &hotkey)
            });
            match copied_detail(lang, smart_paste, meta.hint) {
                Some(detail) => crate::notice::toast_with_detail(app, tr(lang, "toast.copied"), &detail),
                None => crate::notice::toast(app, tr(lang, "toast.copied")),
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
        Finished::AddedThenPicture => {
            // Counted once the picture is on the clipboard, like a set copy that worked.
            nudge = false;
            set_last_copy(app, None);
            after_set_changed(app);
            let app = app.clone();
            let marks = marks.len() as u32;
            std::thread::spawn(move || match copy_picture(&app) {
                Ok(cramped) => {
                    picture_toast(&app, lang, cramped);
                    record_and_maybe_nudge(&app, marks);
                }
                // No fallback to the album: the user picked this mode. The snapshot is in the set already.
                Err(err) => {
                    log::error!("one picture failed: {err}");
                    crate::notice::toast_error(&app, tr(lang, "toast.pictureFailed"), Some(&err));
                }
            });
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

/// Second line of the "Copied" toast: the smart paste hint wins over the no-upload note (Lightshot's Ctrl+D).
fn copied_detail(lang: Lang, smart_paste: Option<String>, hint: Option<FinishHint>) -> Option<String> {
    smart_paste.or_else(|| {
        (hint == Some(FinishHint::NoUpload)).then(|| tr(lang, "toast.noUpload").replace("{paste}", PASTE_KEYS))
    })
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
    // After the strip: for the first snapshot it is created here, and it picks the request up once loaded.
    crate::sheet::request_prewarm(app);
    crate::hotkeys::apply(app);
    crate::tray::refresh(app);
}

fn one_picture(app: &AppHandle) -> bool {
    app.state::<AppState>().settings().set_copy_mode == SetCopyMode::OnePicture
}

/// The whole set as one picture, bitmap only: the comments are drawn under each snapshot, text would repeat
/// them. Returns whether the picture is cramped. Blocks until the strip has built it.
fn copy_picture(app: &AppHandle) -> Result<bool, String> {
    let sheet = crate::sheet::ensure(app, None, true)?;
    clipboard::copy_image(sheet.png.to_vec(), None).map_err(|e| e.to_string())?;
    log::info!("set copied as one {}x{} picture", sheet.width, sheet.height);
    Ok(sheet.cramped)
}

fn picture_toast(app: &AppHandle, lang: Lang, cramped: bool) {
    let text = tr(lang, "toast.copiedPicture");
    if cramped {
        crate::notice::toast_with_detail(app, text, tr(lang, "toast.pictureCramped"));
    } else {
        crate::notice::toast(app, text);
    }
}

/// Strip "Copy all": the set becomes what smart paste pastes, even if a single copy came after it. Off the
/// async workers: one picture waits for the strip, which needs a worker to deliver it.
pub async fn copy_all(app: &AppHandle) -> Result<(), String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || copy_all_blocking(&app)).await.map_err(|e| e.to_string())?
}

fn copy_all_blocking(app: &AppHandle) -> Result<(), String> {
    let lang = app.state::<AppState>().lang();
    if one_picture(app) {
        let cramped = copy_picture(app)?;
        set_last_copy(app, None);
        picture_toast(app, lang, cramped);
    } else {
        copy_set_to_clipboard(app)?;
        set_last_copy(app, None);
        crate::notice::toast(app, tr(lang, "toast.copiedAll"));
    }
    crate::telemetry::count("set_copy_all");
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

/// Strip "Save all": every snapshot as its own file, or in one-picture mode the set as one PNG. Returns the
/// number of snapshots saved.
pub async fn save_all(app: &AppHandle) -> Result<usize, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || save_all_blocking(&app)).await.map_err(|e| e.to_string())?
}

fn save_all_blocking(app: &AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let dir = save_dir(app);
    let stem = export::timestamp_stem(chrono::Local::now());
    if one_picture(app) {
        let letters = state.session.lock().unwrap().as_ref().map(Session::letters).unwrap_or_default();
        if letters.is_empty() {
            return Ok(0);
        }
        let sheet = crate::sheet::ensure(app, Some(&letters), true)?;
        // Always PNG, like the album files: the sheet is text and UI.
        let path = export::save_unique(&dir, &picture_stem(&stem, &letters), "png", &sheet.png).map_err(|e| e.to_string())?;
        crate::notice::toast_with_detail(app, tr(state.lang(), "toast.saved"), &path.to_string_lossy());
        record_and_maybe_nudge(app, 0);
        return Ok(letters.len());
    }
    let snapshots = state.session.lock().unwrap().as_ref().map(|s| s.snapshots.clone()).unwrap_or_default();
    for snap in &snapshots {
        let bytes = std::fs::read(&snap.path).map_err(|e| e.to_string())?;
        export::save_unique(&dir, &format!("{stem}_{}", snap.letter), "png", &bytes).map_err(|e| e.to_string())?;
    }
    if !snapshots.is_empty() {
        crate::telemetry::count("set_save_all");
        crate::notice::toast_with_detail(app, tr(state.lang(), "toast.saved"), &dir.to_string_lossy());
        record_and_maybe_nudge(app, 0);
    }
    Ok(snapshots.len())
}

/// "WellSnap_..._A-F" for a saved picture of a set, "WellSnap_..._A" for one snapshot.
fn picture_stem(stem: &str, letters: &[char]) -> String {
    format!("{stem}_{}", crate::sheet::letter_range(letters))
}

pub fn clear_set(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cleared = state.session.lock().unwrap().take();
    if let Some(session) = cleared {
        log::info!("set {} cleared", session.id);
        crate::telemetry::count("set_cleared");
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

/// Deletes a removed snapshot file, a cleared set folder or an evicted sheet after the grace period, unless it
/// belongs to the current set by then.
pub(crate) fn delete_later(app: &AppHandle, path: PathBuf) {
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

/// Where smart paste takes the pictures and the text from.
enum Source {
    /// The last single copy: its bitmap and comments. Never joined, also in one-picture mode.
    LastCopy { png: Vec<u8>, text: String },
    /// The snapshot set, as it was when the paste began.
    Set(Session),
}

/// What smart paste puts on the clipboard for one `Step::Content`.
enum PasteContent {
    /// Files in paste order: the snapshots, or a joined sheet in place of each group (album, attachments).
    Files(Vec<PathBuf>),
    /// One bitmap: a single copy, a snapshot or a joined sheet.
    Image(Vec<u8>),
    /// The "Copy all" payload for targets that read HTML: files, text and HTML with the pictures embedded.
    CopyAll { files: Vec<PathBuf>, text: String, html: String },
}

impl PasteContent {
    /// Files and bitmaps go without text and HTML: the text was pasted in its own step, and targets that read
    /// both (Word, or Chromium with a bitmap) would show it twice.
    fn write(&self) -> Result<(), String> {
        match self {
            PasteContent::Files(files) => clipboard::copy_files(files.clone(), None, None),
            PasteContent::Image(png) => clipboard::copy_image(png.clone(), None),
            PasteContent::CopyAll { files, text, html } => {
                clipboard::copy_files(files.clone(), Some(text.clone()), Some(html.clone()))
            }
        }
        .map_err(|e| e.to_string())
    }
}

/// What a smart paste sends, decided before anything is built or pressed.
#[derive(Debug, PartialEq)]
struct Decision {
    shape: Shape,
    /// The text to paste, with the legend line in front where the target asks for it.
    text: String,
    steps: Vec<Step>,
}

/// `letters` is None for a single copy: one bitmap, which one-picture mode leaves alone.
fn decide(
    lang: Lang,
    profile: &Profile,
    letters: Option<&[char]>,
    text: &str,
    html_has_images: bool,
    one_picture_mode: bool,
) -> Decision {
    let shape = match letters {
        Some(letters) => plan::shape(profile, letters, html_has_images, one_picture_mode),
        None if matches!(profile.class, Class::TextOnly | Class::Manual) => Shape::None,
        None => Shape::Picture,
    };
    let pictures = letters.map(|letters| plan::pictures(&shape, letters)).unwrap_or_default();
    let text = plan::text_with_legend(lang, text, profile, &pictures);
    let steps = plan::plan(&text, &shape, letters.map_or(1, <[char]>::len), profile);
    Decision { shape, text, steps }
}

/// One action of a paste. `acts` expands the steps into these, so their order is tested without a clipboard or
/// a keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// `SMART_PASTE_GAP` between two steps.
    Pause,
    /// The text on the clipboard. `transient` keeps it out of the clipboard history when pictures replace it
    /// right away; text that is the whole result (a plain text editor) stays in the history.
    PutText { transient: bool },
    /// Content `k` on the clipboard.
    Put(usize),
    /// Stop when the window changed. Before every key press, after the clipboard write: writing a picture can
    /// take a while, and the notice can then say the pictures are on the clipboard.
    CheckFocus,
    /// Ctrl+V (Cmd+V on macOS). Smart paste never presses Enter.
    Paste,
    /// The text stays on the clipboard for the user; no key press.
    LeaveText,
}

fn acts(steps: &[Step]) -> Vec<Act> {
    let mut acts = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            acts.push(Act::Pause);
        }
        let put = match *step {
            Step::Text => {
                let replaced = steps[i + 1..].iter().any(|later| matches!(later, Step::Content(_)));
                Act::PutText { transient: replaced }
            }
            Step::Content(k) => Act::Put(k),
            Step::TextToClipboard => {
                acts.push(Act::LeaveText);
                continue;
            }
        };
        acts.push(put);
        acts.push(Act::CheckFocus);
        acts.push(Act::Paste);
    }
    acts
}

/// How running the acts ended.
#[derive(Debug, Default, PartialEq, Eq)]
struct Ran {
    /// The window changed after `done` of `total` contents were pasted.
    stopped: Option<(usize, usize)>,
    /// The window changed before the first key press, with only the text on the clipboard.
    cancelled: bool,
    /// The text was left on the clipboard.
    text_left: bool,
}

/// A window change stops the paste without an error; a failed clipboard write or key press is an error.
fn run(acts: &[Act], text: &str, contents: &[PasteContent], still_there: impl Fn() -> bool) -> Result<Ran, String> {
    let mut ran = Ran::default();
    let (mut done, mut content_put, mut pressed) = (0, false, false);
    for act in acts {
        match *act {
            Act::Pause => std::thread::sleep(SMART_PASTE_GAP),
            Act::PutText { transient } => {
                clipboard::copy_text(text.to_string(), transient).map_err(|e| e.to_string())?;
                content_put = false;
            }
            Act::Put(k) => {
                contents.get(k).ok_or("no content to paste")?.write()?;
                content_put = true;
            }
            // Focus moved since the last check (a notification click, Alt+Tab): the rest would go elsewhere.
            Act::CheckFocus => {
                if !still_there() {
                    if content_put || pressed {
                        ran.stopped = Some((done, contents.len()));
                    } else {
                        ran.cancelled = true;
                    }
                    return Ok(ran);
                }
            }
            Act::Paste => {
                crate::paste::send_paste()?;
                done += usize::from(content_put);
                pressed = true;
            }
            Act::LeaveText => {
                clipboard::copy_text(text.to_string(), false).map_err(|e| e.to_string())?;
                ran.text_left = true;
            }
        }
    }
    Ok(ran)
}

/// The notice after the steps ran, as text and detail. None when the paste went as usual.
fn notice_after(
    lang: Lang,
    ran: &Ran,
    target: &Target,
    profile: &Profile,
    shape: &Shape,
    snapshots: usize,
    cramped: bool,
) -> Option<(String, Option<&'static str>)> {
    if ran.cancelled {
        return Some((tr(lang, "toast.pasteCancelled").to_string(), None));
    }
    if let Some((done, total)) = ran.stopped {
        let text = if total > 1 {
            tr(lang, "toast.pasteStopped").replace("{done}", &done.to_string()).replace("{total}", &total.to_string())
        } else {
            tr(lang, "toast.windowChanged").to_string()
        };
        return Some((text, None));
    }
    if profile.class == Class::Manual {
        let detail = ran.text_left.then(|| tr(lang, "toast.pasteManualText"));
        return Some((tr(lang, "toast.pasteManual").to_string(), detail));
    }
    // There is always at least one picture behind a paste, and this class drops them all.
    if profile.class == Class::TextOnly {
        return Some((tr(lang, "toast.pasteTextOnly").to_string(), None));
    }
    // Whatever the class: one-picture mode and the rich-HTML fallback join the set too.
    let cramped = (*shape == Shape::Picture && cramped).then(|| tr(lang, "toast.pictureCramped"));
    if ran.text_left {
        return Some((tr(lang, "toast.textInClipboard").to_string(), cramped));
    }
    match shape {
        Shape::Grouped(_) => {
            let max = profile.max_items.unwrap_or_default().to_string();
            Some((tr(lang, "toast.pasteGrouped").replace("{app}", target.name(lang)).replace("{max}", &max), None))
        }
        Shape::Picture if profile.class == Class::OnePicture && snapshots >= 2 => {
            Some((tr(lang, "toast.pasteJoined").to_string(), cramped))
        }
        _ => cramped.map(|detail| (tr(lang, "toast.pastedPicture").to_string(), Some(detail))),
    }
}

/// Registry rows of desktop messengers and mail programs, for the usage counters.
const MESSENGERS: &[&str] = &["whatsapp", "whatsapp_mac", "viber", "zoom", "discord", "slack", "teams"];
const MAIL: &[&str] =
    &["outlook_classic", "outlook_new", "outlook_mac", "thunderbird", "emclient", "thebat", "apple_mail"];

/// The usage counter of a paste that went through: only the kind of program, from the detected row (a planned
/// row counts too). A page in a browser counts as the browser, whatever its title says.
fn ok_counter(detected: &Detected) -> &'static str {
    let id = detected.target.id;
    if detected.engine != Engine::Native {
        "paste_ok_browser"
    } else if id == "tg_desktop" || id == "tg_fork" {
        "paste_ok_telegram"
    } else if MESSENGERS.contains(&id) {
        "paste_ok_messenger"
    } else if MAIL.contains(&id) {
        "paste_ok_mail"
    } else if id == "word" {
        "paste_ok_office"
    } else {
        "paste_ok_other"
    }
}

/// A smart paste that failed: the usage counter, the notice (an i18n key) and the reason under it.
struct PasteError {
    counter: &'static str,
    notice: &'static str,
    reason: String,
}

impl PasteError {
    fn new(counter: &'static str, reason: &str) -> Self {
        PasteError { counter, notice: "toast.pasteFailed", reason: reason.to_string() }
    }

    /// A joined picture could not be built. Nothing is pasted: files instead would give Firefox only the first
    /// one, and a target that takes one picture only one snapshot.
    fn picture(reason: String) -> Self {
        PasteError { counter: "paste_fail_other", notice: "toast.pictureFailed", reason }
    }
}

impl From<String> for PasteError {
    fn from(reason: String) -> Self {
        PasteError { counter: "paste_fail_other", notice: "toast.pasteFailed", reason }
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
        let counter = match smart_paste_blocking(&app) {
            Ok(counter) => counter,
            Err(err) => {
                log::error!("smart paste failed: {}", err.reason);
                let state = app.state::<AppState>();
                crate::notice::toast_error(&app, tr(state.lang(), err.notice), Some(&err.reason));
                err.counter
            }
        };
        crate::telemetry::count(counter);
    });
}

/// Returns the usage counter of the outcome.
fn smart_paste_blocking(app: &AppHandle) -> Result<&'static str, PasteError> {
    let state = app.state::<AppState>();
    let lang = state.lang();
    // A single copy is cleared by every later finish and by "Copy all", so when it exists it is the newest.
    let last = state
        .last_copy
        .lock()
        .unwrap()
        .as_ref()
        .map(|(_, c)| Source::LastCopy { png: c.png.clone(), text: c.text.clone() });
    let source = match last {
        Some(found) => found,
        None => {
            let guard = state.session.lock().unwrap();
            let session = guard.as_ref().filter(|s| !s.is_empty());
            Source::Set(session.ok_or_else(|| PasteError::new("paste_fail_nothing", "nothing to paste"))?.clone())
        }
    };
    if !crate::paste::can_send_input() {
        crate::paste::request_input_permission();
        return Err(PasteError::new("paste_fail_no_permission", "input permission is missing"));
    }
    // A synthetic Ctrl+V combined with held hotkey keys would be another chord (or this hotkey again).
    if !crate::paste::wait_modifiers_released(MODIFIERS_RELEASE_TIMEOUT) {
        return Err(PasteError::new("paste_fail_modifiers_held", "release the hotkey keys and try again"));
    }
    // Input to a window of higher integrity is dropped without an error; leave the clipboard untouched.
    if crate::paste::foreground_is_elevated() {
        return Err(PasteError::new(
            "paste_fail_elevated",
            "the target window runs as administrator; WellSnap cannot paste into it",
        ));
    }
    let detected = target::detect(&crate::paste::foreground_info().unwrap_or_default());
    let (target, profile) = target::effective(&detected, ALLOW_PLANNED);
    let window = crate::paste::foreground_id();
    let title = (TITLE_FINGERPRINT && profile.class == Class::Sequential).then(crate::paste::foreground_fingerprint);
    let still_there = || {
        crate::paste::foreground_id() == window
            && title.as_ref().is_none_or(|title| crate::paste::foreground_fingerprint() == *title)
    };

    let (decision, snapshots, html) = match &source {
        Source::LastCopy { text, .. } => (decide(lang, &profile, None, text, false, false), 1, None),
        Source::Set(session) => {
            let letters = session.letters();
            let one_picture_mode = one_picture(app);
            // Reads every file, so only for a target that takes the HTML.
            let html = (profile.class == Class::RichHtml && !one_picture_mode)
                .then(|| session.html_block_with_images(lang));
            let has_images = html.as_ref().is_some_and(|(_, complete)| *complete);
            let text = session.text_block(lang);
            let decision = decide(lang, &profile, Some(&letters), &text, has_images, one_picture_mode);
            (decision, letters.len(), html.map(|(html, _)| html))
        }
    };
    log::info!(
        "smart paste: target={} class={:?} items={snapshots} steps={}",
        target.id,
        profile.class,
        decision.steps.len()
    );
    // A plain text editor and no comments: no key press would do anything.
    if decision.steps.is_empty() && profile.class == Class::TextOnly {
        crate::notice::toast(app, tr(lang, "toast.pasteNoText"));
        return Ok("paste_fail_nothing");
    }

    // No "building" notice: showing the notice window can activate it and take the focus from the target.
    let (contents, cramped) = build_contents(app, &source, &decision.shape, &decision.text, html)?;
    // Building a sheet takes a while; the user may have moved on.
    if !still_there() {
        crate::notice::toast(app, tr(lang, "toast.pasteCancelled"));
        return Ok("paste_fail_window_changed");
    }
    let ran = run(&acts(&decision.steps), &decision.text, &contents, still_there)?;
    match notice_after(lang, &ran, target, &profile, &decision.shape, snapshots, cramped) {
        Some((text, Some(detail))) => crate::notice::toast_with_detail(app, &text, detail),
        Some((text, None)) => crate::notice::toast(app, &text),
        None => {}
    }
    if ran.stopped.is_some() || ran.cancelled {
        return Ok("paste_fail_window_changed");
    }
    if ran.text_left && profile.class != Class::Manual {
        crate::telemetry::count("paste_text_split");
    }
    Ok(ok_counter(&detected))
}

/// The clipboard contents of `shape`, one per `Step::Content`, and whether a joined picture of the whole set is
/// cramped. Every sheet is built here, before the first key press, so the pause between the text and the
/// pictures stays `SMART_PASTE_GAP`.
fn build_contents(
    app: &AppHandle,
    source: &Source,
    shape: &Shape,
    text: &str,
    html: Option<String>,
) -> Result<(Vec<PasteContent>, bool), PasteError> {
    let session = match source {
        Source::LastCopy { .. } if *shape == Shape::None => return Ok((Vec::new(), false)),
        Source::LastCopy { png, .. } => return Ok((vec![PasteContent::Image(png.clone())], false)),
        Source::Set(session) => session,
    };
    // The comments go as text, so smart paste's sheets leave them out.
    let sheet = |letters: &[char]| crate::sheet::ensure(app, Some(letters), false).map_err(PasteError::picture);
    let file = |letter: char| {
        session.get(letter).map(|s| s.path.clone()).ok_or_else(|| format!("snapshot {letter} is no longer in the set"))
    };
    // A group of one keeps its own file.
    let bitmap = |group: &[char]| -> Result<PasteContent, PasteError> {
        match group {
            [letter] => Ok(PasteContent::Image(std::fs::read(file(*letter)?).map_err(|e| e.to_string())?)),
            _ => Ok(PasteContent::Image(sheet(group)?.png.to_vec())),
        }
    };
    let letters = session.letters();
    Ok(match shape {
        Shape::Files => (vec![PasteContent::Files(session.files())], false),
        Shape::Grouped(groups) => {
            let files = groups
                .iter()
                .map(|group| match group.as_slice() {
                    [letter] => file(*letter).map_err(PasteError::from),
                    _ => sheet(group).map(|sheet| sheet.path),
                })
                .collect::<Result<_, _>>()?;
            (vec![PasteContent::Files(files)], false)
        }
        Shape::Each => (letters.iter().map(|&letter| bitmap(&[letter])).collect::<Result<_, _>>()?, false),
        Shape::EachGrouped(groups) => (groups.iter().map(|group| bitmap(group)).collect::<Result<_, _>>()?, false),
        Shape::CopyAll => {
            let html = html.unwrap_or_default();
            (vec![PasteContent::CopyAll { files: session.files(), text: text.to_string(), html }], false)
        }
        Shape::Picture => {
            let sheet = sheet(&letters)?;
            (vec![PasteContent::Image(sheet.png.to_vec())], sheet.cramped)
        }
        Shape::None => (Vec::new(), false),
    })
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
    fn copied_toast_explains_no_upload_unless_smart_paste_has_a_hint() {
        let smart = Some("Ctrl+Alt+V: paste".to_string());
        assert_eq!(copied_detail(Lang::En, smart.clone(), Some(FinishHint::NoUpload)), smart);
        assert_eq!(copied_detail(Lang::En, None, None), None);
        let note = copied_detail(Lang::Ru, None, Some(FinishHint::NoUpload)).unwrap();
        assert!(note.starts_with("WellSnap ничего не загружает") && note.contains(PASTE_KEYS) && !note.contains("{paste}"));
    }

    #[test]
    fn a_saved_picture_is_named_after_its_letters() {
        assert_eq!(picture_stem("WellSnap_2026-10-22_14-05-33", &['A', 'B', 'F']), "WellSnap_2026-10-22_14-05-33_A-F");
        assert_eq!(picture_stem("WellSnap_2026-10-22_14-05-33", &['A']), "WellSnap_2026-10-22_14-05-33_A");
    }

    /// The three paste orders before paste targets existed.
    const CONTENT_ONLY: [Act; 3] = [Act::Put(0), Act::CheckFocus, Act::Paste];
    const TEXT_THEN_CONTENT: [Act; 7] = [
        Act::PutText { transient: true },
        Act::CheckFocus,
        Act::Paste,
        Act::Pause,
        Act::Put(0),
        Act::CheckFocus,
        Act::Paste,
    ];
    const CONTENT_THEN_TEXT: [Act; 5] = [Act::Put(0), Act::CheckFocus, Act::Paste, Act::Pause, Act::LeaveText];

    fn letters(n: usize) -> Vec<char> {
        (0..n).map(|i| (b'A' + i as u8) as char).collect()
    }

    /// The target and profile smart paste uses for this foreground program, as in every build.
    fn profile_of(app: &str, title: Option<&str>) -> (&'static str, Profile) {
        let fg = target::ForegroundInfo { app: app.into(), path: String::new(), title: title.map(str::to_string) };
        let (target, profile) = target::effective(&target::detect(&fg), ALLOW_PLANNED);
        (target.id, profile)
    }

    fn planned(id: &str) -> Profile {
        target::by_id(id).unwrap().profile
    }

    fn set_acts(profile: &Profile, n: usize, text: &str) -> Vec<Act> {
        acts(&decide(Lang::Ru, profile, Some(&letters(n)), text, true, false).steps)
    }

    fn single_acts(profile: &Profile, text: &str) -> Vec<Act> {
        acts(&decide(Lang::Ru, profile, None, text, false, false).steps)
    }

    /// The old `plan(text, items, telegram)` cases, with the profiles detected for Telegram Desktop and for an
    /// unknown program.
    #[test]
    fn telegram_and_unknown_programs_paste_as_before() {
        let (id, telegram) = profile_of("Telegram.exe", None);
        let limits = (telegram.class, telegram.caption_max, telegram.album_max);
        assert_eq!((id, limits), ("tg_desktop", (Class::Album, Some(1024), Some(10))));
        assert_eq!(profile_of("ru.keepcoder.Telegram", None).0, "tg_desktop");
        let (id, unknown) = profile_of("someapp.exe", None);
        assert_eq!((id, unknown), (target::UNKNOWN, Profile::DEFAULT));

        assert_eq!(set_acts(&telegram, 3, ""), CONTENT_ONLY);
        assert_eq!(set_acts(&unknown, 3, "A1 x"), TEXT_THEN_CONTENT);
        assert_eq!(set_acts(&telegram, 3, "A1 x"), TEXT_THEN_CONTENT);
        assert_eq!(set_acts(&telegram, 10, "A1 x"), TEXT_THEN_CONTENT);
        assert_eq!(set_acts(&telegram, 11, "A1 x"), CONTENT_THEN_TEXT);
        assert_eq!(set_acts(&unknown, 11, "A1 x"), TEXT_THEN_CONTENT);
        let long = "я".repeat(1025);
        assert_eq!(single_acts(&telegram, &long), CONTENT_THEN_TEXT);
        assert_eq!(set_acts(&telegram, 1, &long), CONTENT_THEN_TEXT);
        assert_eq!(single_acts(&telegram, &"я".repeat(1024)), TEXT_THEN_CONTENT);
        assert_eq!(single_acts(&unknown, &long), TEXT_THEN_CONTENT);
        assert_eq!(single_acts(&unknown, ""), CONTENT_ONLY);
        // Album mode: every snapshot as one file list, the text as it is.
        for profile in [telegram, unknown] {
            let decision = decide(Lang::Ru, &profile, Some(&letters(26)), "A1 x", true, false);
            assert_eq!((decision.shape, decision.text.as_str()), (Shape::Files, "A1 x"));
        }
        let single = decide(Lang::Ru, &telegram, None, "1 x", false, false);
        assert_eq!(single.shape, Shape::Picture);
    }

    #[test]
    fn planned_targets_paste_like_unknown_programs_in_every_build() {
        for (app, title) in [
            ("slack.exe", None),
            ("Code.exe", None),
            ("notepad++.exe", None),
            ("com.apple.TextEdit", None),
            ("chrome.exe", Some("Inbox - Gmail - Google Chrome")),
            ("chrome.exe", Some("Chat - Claude - Google Chrome")),
            ("firefox.exe", Some("Page \u{2014} Mozilla Firefox")),
        ] {
            assert_eq!(profile_of(app, title), (target::UNKNOWN, Profile::DEFAULT), "{app}");
        }
    }

    #[test]
    fn targets_checked_live_use_their_profiles() {
        assert_eq!(profile_of("notepad.exe", None).0, "text_editor");
        assert_eq!(profile_of("WhatsApp.Root.exe", None).0, "whatsapp");
        assert_eq!(profile_of("WINWORD.EXE", None).0, "word");
        assert_eq!(profile_of("MailClient.exe", None).0, "emclient");
        for app in ["WhatsApp.Root.exe", "WINWORD.EXE", "MailClient.exe"] {
            assert_eq!(profile_of(app, None).1.class, Class::Attach, "{app}");
        }
        assert_eq!(profile_of("notepad.exe", None).1.class, Class::TextOnly);
    }

    #[test]
    fn terminals_never_get_a_key_press() {
        let (id, terminal) = profile_of("WindowsTerminal.exe", None);
        assert_eq!((id, terminal.class), ("terminal", Class::Manual));
        for one_picture_mode in [false, true] {
            let decision = decide(Lang::En, &terminal, Some(&letters(3)), "A1 x", true, one_picture_mode);
            assert_eq!((&decision.shape, acts(&decision.steps)), (&Shape::None, vec![Act::LeaveText]));
        }
        let single = decide(Lang::En, &terminal, None, "1 x", false, false);
        assert_eq!((single.shape, acts(&single.steps)), (Shape::None, vec![Act::LeaveText]));
        assert!(set_acts(&terminal, 3, "").is_empty());
        assert!(single_acts(&terminal, "").is_empty());
    }

    #[test]
    fn one_picture_mode_joins_the_set_and_keeps_the_target_rules() {
        let (_, telegram) = profile_of("Telegram.exe", None);
        let all = letters(12);
        let joined = decide(Lang::Ru, &telegram, Some(&all), "A1 x", false, true);
        assert_eq!((joined.shape, acts(&joined.steps)), (Shape::Picture, TEXT_THEN_CONTENT.to_vec()));
        let long = decide(Lang::Ru, &telegram, Some(&all), &"я".repeat(1025), false, true);
        assert_eq!(acts(&long.steps), CONTENT_THEN_TEXT, "the caption limit still applies");
        let unknown = decide(Lang::Ru, &Profile::DEFAULT, Some(&all), "A1 x", true, true);
        assert_eq!((unknown.shape, acts(&unknown.steps)), (Shape::Picture, TEXT_THEN_CONTENT.to_vec()));
        // A single copy is one bitmap in either mode.
        let single = |one_picture_mode| decide(Lang::Ru, &telegram, None, "1 x", false, one_picture_mode);
        assert_eq!(single(true), single(false));
    }

    #[test]
    fn ai_chats_get_the_legend_and_joined_groups() {
        let claude = planned("claude");
        let three = decide(Lang::En, &claude, Some(&letters(3)), "Screenshot A\nA1 x", true, false);
        let legend = "Images in order: 1) Screenshot A; 2) Screenshot B; 3) Screenshot C";
        assert_eq!(three.text, format!("{legend}\n\nScreenshot A\nA1 x"));
        assert_eq!((three.shape, acts(&three.steps)), (Shape::Files, TEXT_THEN_CONTENT.to_vec()));
        let all = decide(Lang::En, &claude, Some(&letters(26)), "A1 x", true, false);
        assert_eq!(all.shape, Shape::Grouped(plan::groups(&letters(26), 20)));
        assert!(all.text.starts_with("Images in order: 1) Screenshots A, B; 2) Screenshots C, D;"), "{}", all.text);
        assert_eq!(acts(&all.steps), TEXT_THEN_CONTENT, "one file list");
        assert_eq!(decide(Lang::En, &claude, Some(&letters(3)), "", true, false).text, "", "no text, no legend");
        assert_eq!(decide(Lang::En, &claude, None, "1 x", false, false).text, "1 x", "a single copy is one picture");
        assert_eq!(decide(Lang::En, &claude, Some(&letters(3)), "A1 x", true, true).text, "A1 x", "one picture mode");
    }

    #[test]
    fn rich_html_and_sequential_targets() {
        let thunderbird = planned("thunderbird");
        let rich = decide(Lang::En, &thunderbird, Some(&letters(3)), "A1 x", true, false);
        let expected = (Shape::CopyAll, CONTENT_ONLY.to_vec());
        assert_eq!((rich.shape, acts(&rich.steps)), expected, "the payload has the text");
        let too_large = decide(Lang::En, &thunderbird, Some(&letters(3)), "A1 x", false, false);
        assert_eq!((too_large.shape, acts(&too_large.steps)), (Shape::Picture, TEXT_THEN_CONTENT.to_vec()));

        let gmail = planned("gmail");
        let each = |k| [Act::Pause, Act::Put(k), Act::CheckFocus, Act::Paste];
        let text = vec![Act::PutText { transient: true }, Act::CheckFocus, Act::Paste];
        let expected = [text, each(0).to_vec(), each(1).to_vec(), each(2).to_vec()].concat();
        assert_eq!(set_acts(&gmail, 3, "A1 x"), expected);
        assert_eq!(set_acts(&gmail, 2, ""), [CONTENT_ONLY.to_vec(), each(1).to_vec()].concat());
        let many = decide(Lang::En, &gmail, Some(&letters(26)), "A1 x", true, false);
        assert_eq!(acts(&many.steps).iter().filter(|a| **a == Act::Paste).count(), 1 + plan::SEQUENTIAL_MAX);

        let notepad = planned("text_editor");
        let text = [Act::PutText { transient: false }, Act::CheckFocus, Act::Paste];
        assert_eq!(set_acts(&notepad, 3, "A1 x"), text, "the result");
        assert!(set_acts(&notepad, 3, "").is_empty());
    }

    #[test]
    fn every_key_press_checks_the_window() {
        let late_text = acts(&[Step::Content(0), Step::Text]);
        let expected = [
            Act::Put(0),
            Act::CheckFocus,
            Act::Paste,
            Act::Pause,
            Act::PutText { transient: false },
            Act::CheckFocus,
            Act::Paste,
        ];
        assert_eq!(late_text, expected);
        for steps in [vec![Step::Text, Step::Content(0), Step::Content(1)], vec![Step::Content(0), Step::Text]] {
            let acts = acts(&steps);
            let presses = acts.iter().enumerate().filter(|(_, act)| **act == Act::Paste).map(|(i, _)| i);
            for i in presses {
                assert_eq!(acts[i - 1], Act::CheckFocus, "{steps:?}");
                assert!(matches!(acts[i - 2], Act::Put(_) | Act::PutText { .. }), "written before the check");
            }
        }
    }

    #[test]
    fn notices_after_the_paste() {
        let notice = |ran: &Ran, id: &str, shape: &Shape, snapshots: usize, cramped: bool| {
            let target = target::by_id(id).unwrap();
            notice_after(Lang::En, ran, target, &target.profile, shape, snapshots, cramped)
        };
        let went = Ran::default();
        let stopped = |done, total| Ran { stopped: Some((done, total)), ..Ran::default() };
        let text = |key: &str| Some((tr(Lang::En, key).to_string(), None));
        assert_eq!(notice(&went, target::UNKNOWN, &Shape::Files, 3, false), None);
        assert_eq!(notice(&stopped(0, 1), target::UNKNOWN, &Shape::Files, 3, false), text("toast.windowChanged"));
        let cancelled = Ran { cancelled: true, ..Ran::default() };
        assert_eq!(notice(&cancelled, target::UNKNOWN, &Shape::Files, 3, false), text("toast.pasteCancelled"));
        assert_eq!(notice(&went, "text_editor", &Shape::None, 3, false), text("toast.pasteTextOnly"));
        assert_eq!(notice(&cancelled, "text_editor", &Shape::None, 3, false), text("toast.pasteCancelled"));
        assert_eq!(
            notice(&stopped(2, 10), "gmail", &Shape::Each, 10, false).unwrap().0,
            "The window changed, paste stopped: 2 of 10 pasted"
        );
        let text_left = Ran { text_left: true, ..Ran::default() };
        assert_eq!(notice(&text_left, "tg_desktop", &Shape::Files, 11, false), text("toast.textInClipboard"));
        assert_eq!(
            notice(&text_left, "terminal", &Shape::None, 3, false),
            Some((tr(Lang::En, "toast.pasteManual").to_string(), Some(tr(Lang::En, "toast.pasteManualText"))))
        );
        assert_eq!(notice(&went, "terminal", &Shape::None, 3, false), text("toast.pasteManual"), "no text");
        let grouped = Shape::Grouped(plan::groups(&letters(26), 20));
        assert_eq!(
            notice(&went, "claude", &grouped, 26, false).unwrap().0,
            "Claude takes up to 20 images at a time, so some screenshots were joined into shared images"
        );
        assert_eq!(notice(&went, "vscode", &Shape::Picture, 3, false), text("toast.pasteJoined"));
        assert_eq!(
            notice(&went, "vscode", &Shape::Picture, 26, true),
            Some((tr(Lang::En, "toast.pasteJoined").to_string(), Some(tr(Lang::En, "toast.pictureCramped"))))
        );
        assert_eq!(notice(&went, "vscode", &Shape::Picture, 1, false), None, "a single copy");
        assert_eq!(notice(&went, "tg_desktop", &Shape::Picture, 12, false), None, "one-picture mode");
        // A cramped picture is reported whatever the class: one-picture mode, the rich-HTML fallback.
        let cramped = Some(tr(Lang::En, "toast.pictureCramped"));
        let pasted = Some((tr(Lang::En, "toast.pastedPicture").to_string(), cramped));
        assert_eq!(notice(&went, "tg_desktop", &Shape::Picture, 26, true), pasted);
        assert_eq!(notice(&went, target::UNKNOWN, &Shape::Picture, 26, true), pasted);
        assert_eq!(notice(&went, "thunderbird", &Shape::Picture, 26, true), pasted);
        let split = Some((tr(Lang::En, "toast.textInClipboard").to_string(), cramped));
        assert_eq!(notice(&text_left, "tg_desktop", &Shape::Picture, 26, true), split);
        assert_eq!(notice(&went, target::UNKNOWN, &Shape::Files, 26, true), None, "only a joined picture");
    }

    #[test]
    fn paste_counters_name_the_kind_of_program_only() {
        let counter = |app: &str, title: Option<&str>| {
            ok_counter(&target::detect(&target::ForegroundInfo {
                app: app.into(),
                path: String::new(),
                title: title.map(str::to_string),
            }))
        };
        assert_eq!(counter("Telegram.exe", None), "paste_ok_telegram");
        assert_eq!(counter("AyuGram.exe", None), "paste_ok_telegram");
        assert_eq!(counter("WhatsApp.Root.exe", None), "paste_ok_messenger");
        assert_eq!(counter("OUTLOOK.EXE", None), "paste_ok_mail");
        assert_eq!(counter("WINWORD.EXE", None), "paste_ok_office");
        assert_eq!(counter("chrome.exe", Some("Telegram Web - Google Chrome")), "paste_ok_browser");
        assert_eq!(counter("firefox.exe", None), "paste_ok_browser");
        assert_eq!(counter("Code.exe", None), "paste_ok_other");
        assert_eq!(counter("someapp.exe", None), "paste_ok_other");
        for id in MESSENGERS.iter().chain(MAIL) {
            assert!(target::by_id(id).is_some_and(|t| !t.apps.is_empty()), "{id} is a program in the registry");
        }
    }

    #[test]
    fn paste_counters_are_in_the_usage_catalog() {
        let catalog: serde_json::Value = serde_json::from_str(include_str!("telemetry/events.json")).unwrap();
        let counters = catalog["counters"].as_array().unwrap();
        let source = include_str!("actions.rs");
        let pattern = format!("{}paste_", '"');
        let mut found = 0;
        for (i, _) in source.match_indices(&pattern) {
            let rest = &source[i + 1..];
            let name = &rest[..rest.find('"').unwrap()];
            assert!(counters.iter().any(|c| c == name), "{name} is not in events.json");
            found += 1;
        }
        assert!(found >= 12, "the scan found {found} names");
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
