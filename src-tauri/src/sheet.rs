//! "One picture" mode: the strip webview composes every snapshot of the set into one PNG sheet on request.
//! Rust sends `sheet:request` to the strip; the strip answers with `sheet_put` (or `sheet_failed`), and a
//! strip that was still loading asks for the open request with `sheet_pending`.
//!
//! One request is in flight at a time. A caller that needs a sheet waits for the slot (or joins a request for
//! the same sheet), and finished sheets of the current set are cached. Snapshots never change inside a set, so
//! a sheet stays valid while its set has the same id and still holds every letter.

use crate::i18n::tr;
use crate::session::{self, Session, Snapshot};
use crate::settings::SetCopyMode;
use crate::state::AppState;
use crate::windows_ui::STRIP;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Event sent to the strip window with a `SheetRequest` payload.
pub const REQUEST_EVENT: &str = "sheet:request";
/// A request the strip has not answered by then is lost (a crashed or throttled webview); its slot is freed.
pub const SHEET_TIMEOUT: Duration = Duration::from_secs(15);
/// Hard caps of a sheet, the same as in src/strip/sheetLayout.ts: WhatsApp HD long side, and 12 MP keeps the
/// clipboard bitmap (32 bpp) under 48 MB.
pub const MAX_SIDE: u32 = 4096;
pub const MAX_AREA: u64 = 12_000_000;
/// Finished sheets kept for the current set: both prewarmed variants plus the groups of a smart paste.
const CACHE_MAX: usize = 8;
/// Outcomes kept for waiters that have not woken up yet.
const OUTCOMES_MAX: usize = 8;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// A composed sheet, cached under `<set dir>/sheets/<requestId>/`.
#[derive(Debug, Clone)]
pub struct Sheet {
    pub png: Arc<Vec<u8>>,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    /// Too many snapshots to keep the text readable at the size caps.
    pub cramped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetRequest {
    pub request_id: u64,
    pub session_id: String,
    /// Heading word ("Снимок" / "Screenshot"), the same one the text block uses.
    pub title: String,
    /// Draw the comment lines under each snapshot (never for smart paste, which sends the text separately).
    pub with_comments: bool,
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetPutMeta {
    pub request_id: u64,
    pub width: u32,
    pub height: u32,
    pub cramped: bool,
}

/// What a sheet shows: two requests with equal keys produce the same picture.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    session_id: String,
    /// Letter and revision of each snapshot, in sheet order.
    items: Vec<(char, u32)>,
    title: String,
    with_comments: bool,
}

struct Job {
    request: SheetRequest,
    key: Key,
    sent_at: Instant,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    inflight: Option<Job>,
    outcomes: VecDeque<(u64, Result<Sheet, String>)>,
    /// Oldest first.
    cache: Vec<(Key, Sheet)>,
    /// Sheets whose last request failed: a prewarm does not ask for them again (an explicit copy does).
    failed: Vec<Key>,
    /// A prewarm found the slot busy, or had more than one sheet to ask for.
    prewarm_wanted: bool,
}

impl Inner {
    fn cached(&self, key: &Key) -> Option<Sheet> {
        self.cache.iter().find(|(k, _)| k == key).map(|(_, sheet)| sheet.clone())
    }

    fn outcome(&self, id: u64) -> Option<Result<Sheet, String>> {
        self.outcomes.iter().find(|(i, _)| *i == id).map(|(_, result)| result.clone())
    }

    /// Sheets of another set are useless; their files go with that set's folder.
    fn forget_other_sessions(&mut self, session_id: &str) {
        self.cache.retain(|(k, _)| k.session_id == session_id);
        self.failed.retain(|k| k.session_id == session_id);
    }

    fn start(&mut self, key: Key, snapshots: Vec<Snapshot>) -> SheetRequest {
        self.next_id += 1;
        let request = SheetRequest {
            request_id: self.next_id,
            session_id: key.session_id.clone(),
            title: key.title.clone(),
            with_comments: key.with_comments,
            snapshots,
        };
        self.inflight = Some(Job { request: request.clone(), key, sent_at: Instant::now() });
        request
    }

    /// Frees the slot of a request older than `timeout`. True when one was dropped.
    fn expire(&mut self, timeout: Duration) -> bool {
        let id = match &self.inflight {
            Some(job) if job.sent_at.elapsed() >= timeout => job.request.request_id,
            _ => return false,
        };
        log::warn!("sheet request {id} timed out");
        self.settle(id, Err("the strip did not answer in time".into()));
        true
    }

    /// Ends the request in flight with `result`. Returns the folders of evicted sheets, or None when `id` is
    /// not the request in flight.
    fn settle(&mut self, id: u64, result: Result<Sheet, String>) -> Option<Vec<PathBuf>> {
        if self.inflight.as_ref().map(|j| j.request.request_id) != Some(id) {
            return None;
        }
        let job = self.inflight.take()?;
        let mut evicted = Vec::new();
        match &result {
            Ok(sheet) => {
                self.failed.retain(|k| *k != job.key);
                self.cache.retain(|(k, _)| *k != job.key);
                self.cache.push((job.key, sheet.clone()));
                while self.cache.len() > CACHE_MAX {
                    let (_, old) = self.cache.remove(0);
                    evicted.extend(old.path.parent().map(Path::to_path_buf));
                }
            }
            Err(_) => {
                if !self.failed.contains(&job.key) {
                    self.failed.push(job.key);
                }
            }
        }
        self.outcomes.push_back((id, result));
        while self.outcomes.len() > OUTCOMES_MAX {
            self.outcomes.pop_front();
        }
        Some(evicted)
    }
}

/// What the caller of `SheetJobs::finish` still has to do outside the lock.
#[derive(Debug, Default, PartialEq)]
struct Settled {
    /// Folders of sheets that fell out of the cache; deleted after a grace period (a paste may still read them).
    evicted: Vec<PathBuf>,
    /// Run the prewarm again: it skipped a sheet while the slot was busy.
    prewarm: bool,
}

/// One request in flight at a time, plus a small cache of finished sheets for the current set.
#[derive(Default)]
pub struct SheetJobs {
    inner: Mutex<Inner>,
    /// Signalled whenever the request in flight ends.
    changed: Condvar,
}

impl SheetJobs {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, Inner>, deadline: Instant) -> MutexGuard<'a, Inner> {
        let left = deadline.saturating_duration_since(Instant::now());
        self.changed.wait_timeout(guard, left).unwrap_or_else(|e| e.into_inner()).0
    }

    /// The sheet for `key`: from the cache, from the request in flight for the same key, or from a new
    /// request, which `send` delivers (called without the lock). Waits at most `timeout` for the slot and
    /// `timeout` for the strip.
    fn obtain(
        &self,
        key: Key,
        snapshots: Vec<Snapshot>,
        timeout: Duration,
        send: impl FnOnce(&SheetRequest),
    ) -> Result<Sheet, String> {
        let mut inner = self.lock();
        inner.forget_other_sessions(&key.session_id);
        let slot_deadline = Instant::now() + timeout;
        let id = loop {
            if let Some(sheet) = inner.cached(&key) {
                return Ok(sheet);
            }
            if inner.expire(timeout) {
                self.changed.notify_all();
            }
            match &inner.inflight {
                Some(job) if job.key == key => break job.request.request_id,
                Some(job) => {
                    if Instant::now() >= slot_deadline {
                        return Err("another picture is still being built".into());
                    }
                    // Woken when it ends, or at its expiry when the strip never answers.
                    let until = slot_deadline.min(job.sent_at + timeout);
                    inner = self.wait(inner, until);
                }
                None => {
                    let request = inner.start(key.clone(), snapshots);
                    drop(inner);
                    send(&request);
                    inner = self.lock();
                    break request.request_id;
                }
            }
        };
        loop {
            if let Some(result) = inner.outcome(id) {
                return result;
            }
            let Some(sent_at) = inner.inflight.as_ref().filter(|j| j.request.request_id == id).map(|j| j.sent_at)
            else {
                return Err("the picture request was dropped".into());
            };
            if sent_at.elapsed() >= timeout {
                inner.expire(timeout);
                self.changed.notify_all();
                continue;
            }
            inner = self.wait(inner, sent_at + timeout);
        }
    }

    /// Asks for the first sheet of `wanted` that is neither cached, in flight nor failed before, without
    /// waiting. Skipped while another request is in flight; `finish` then reports that it should run again.
    fn prewarm(&self, wanted: Vec<(Key, Vec<Snapshot>)>, timeout: Duration, send: impl FnOnce(&SheetRequest)) {
        let mut inner = self.lock();
        if let Some((key, _)) = wanted.first() {
            inner.forget_other_sessions(&key.session_id);
        }
        if inner.expire(timeout) {
            self.changed.notify_all();
        }
        let mut missing = wanted
            .into_iter()
            .filter(|(key, _)| {
                inner.cached(key).is_none()
                    && !inner.failed.contains(key)
                    && inner.inflight.as_ref().is_none_or(|j| j.key != *key)
            })
            .collect::<Vec<_>>()
            .into_iter();
        let Some((key, snapshots)) = missing.next() else { return };
        let more = missing.next().is_some();
        if inner.inflight.is_some() {
            inner.prewarm_wanted = true;
            return;
        }
        let request = inner.start(key, snapshots);
        inner.prewarm_wanted = more;
        drop(inner);
        send(&request);
    }

    /// Session id and letters of request `id` while it is in flight.
    fn inflight(&self, id: u64) -> Option<(String, Vec<char>)> {
        let inner = self.lock();
        let job = inner.inflight.as_ref().filter(|j| j.request.request_id == id)?;
        Some((job.key.session_id.clone(), job.key.items.iter().map(|(letter, _)| *letter).collect()))
    }

    /// Ends request `id`; None when it is not in flight any more.
    fn finish(&self, id: u64, result: Result<Sheet, String>) -> Option<Settled> {
        let mut inner = self.lock();
        let evicted = inner.settle(id, result)?;
        let prewarm = std::mem::take(&mut inner.prewarm_wanted);
        self.changed.notify_all();
        Some(Settled { evicted, prewarm })
    }

    /// A prewarm skipped a sheet and the slot is free, but no `finish` took the flag: the request it waited for
    /// timed out. Takes the flag, and the caller runs the prewarm.
    fn prewarm_due(&self) -> bool {
        let mut inner = self.lock();
        inner.inflight.is_none() && std::mem::take(&mut inner.prewarm_wanted)
    }

    fn pending(&self, timeout: Duration) -> Option<SheetRequest> {
        let mut inner = self.lock();
        if inner.expire(timeout) {
            self.changed.notify_all();
        }
        inner.inflight.as_ref().map(|j| j.request.clone())
    }

    /// The set is gone: drop the cached pictures (the clipboard or a running paste keeps its own copy).
    fn clear_cache(&self) {
        let mut inner = self.lock();
        inner.cache.clear();
        inner.failed.clear();
    }
}

/// The sheet for `letters` (the whole set when None), built by the strip or taken from the cache. Blocks until
/// the strip answers or the timeout passes; never call it on the main thread.
pub fn ensure(app: &AppHandle, letters: Option<&[char]>, with_comments: bool) -> Result<Sheet, String> {
    let state = app.state::<AppState>();
    let title = tr(state.lang(), "block.snapshot");
    let (key, snapshots) = {
        let guard = state.session.lock().unwrap();
        let session = guard.as_ref().filter(|s| !s.is_empty()).ok_or("the set is empty")?;
        select(session, letters, with_comments, title)?
    };
    let sheet = state.sheets.obtain(key, snapshots, SHEET_TIMEOUT, |request| send(app, request));
    prewarm_if_due(app);
    sheet
}

/// Builds the sheets the next copy will need, without waiting. Only in one-picture mode.
pub fn request_prewarm(app: &AppHandle) {
    let state = app.state::<AppState>();
    let lang = state.lang();
    let one_picture = state.settings().set_copy_mode == SetCopyMode::OnePicture;
    let title = tr(lang, "block.snapshot");
    let wanted = {
        let guard = state.session.lock().unwrap();
        guard.as_ref().filter(|s| !s.is_empty()).map(|session| {
            // Copy all and Save all draw the comments; smart paste sends them as text, so its sheet has none.
            let mut wanted: Vec<(Key, Vec<Snapshot>)> = [true, false]
                .into_iter()
                .filter(|_| one_picture)
                .filter_map(|with_comments| select(session, None, with_comments, title).ok())
                .collect();
            wanted.dedup_by(|a, b| a.0 == b.0);
            wanted
        })
    };
    match wanted {
        Some(wanted) => state.sheets.prewarm(wanted, SHEET_TIMEOUT, |request| send(app, request)),
        None => state.sheets.clear_cache(),
    }
}

/// A sheet from the strip (`sheet_put`).
pub fn accept(app: &AppHandle, meta: SheetPutMeta, png: Vec<u8>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (result, settled) = take_put(&state.sheets, &state.session, &meta, png);
    if let Some(settled) = settled {
        after_finish(app, settled);
    }
    result
}

/// The strip could not compose the request (`sheet_failed`).
pub fn fail(app: &AppHandle, request_id: u64, reason: String) {
    let message = format!("sheet {request_id} failed in the strip: {reason}");
    match app.state::<AppState>().sheets.finish(request_id, Err(reason)) {
        Some(settled) => {
            log::warn!("{message}");
            after_finish(app, settled);
        }
        // The strip also reports a put the core refused, and that already ended the request.
        None => log::debug!("{message} (not in flight)"),
    }
}

/// The request in flight, for a strip that loaded after it was sent (`sheet_pending`).
pub fn pending(app: &AppHandle) -> Option<SheetRequest> {
    let request = app.state::<AppState>().sheets.pending(SHEET_TIMEOUT);
    // The strip has subscribed by now, so a prewarm started here reaches it as an event.
    if request.is_none() {
        prewarm_if_due(app);
    }
    request
}

/// "A-F" for a range, "A" for one snapshot: used in the sheet's file names.
pub fn letter_range(letters: &[char]) -> String {
    match (letters.first(), letters.last()) {
        (Some(first), Some(last)) if first != last => format!("{first}-{last}"),
        (Some(first), _) => first.to_string(),
        _ => String::new(),
    }
}

/// `accept` without the app: checks the put against its request and the set, stores it and ends the request.
fn take_put(
    jobs: &SheetJobs,
    session: &Mutex<Option<Session>>,
    meta: &SheetPutMeta,
    png: Vec<u8>,
) -> (Result<(), String>, Option<Settled>) {
    let id = meta.request_id;
    let Some((session_id, letters)) = jobs.inflight(id) else {
        return (Err(format!("sheet request {id} is not in flight")), None);
    };
    let stored = check_png(&png, meta).and_then(|()| {
        let dir = still_current(session.lock().unwrap().as_ref(), &session_id, &letters)?;
        store(&dir, id, &letters, png, meta)
    });
    conclude(jobs, id, stored)
}

/// Ends request `id` with what the put produced. A request that ended while the file was written (it timed
/// out) refuses the sheet, and its folder goes at once: nothing else knows it.
fn conclude(jobs: &SheetJobs, id: u64, stored: Result<Sheet, String>) -> (Result<(), String>, Option<Settled>) {
    let error = stored.as_ref().err().cloned();
    let folder = stored.as_ref().ok().and_then(|sheet| sheet.path.parent().map(Path::to_path_buf));
    let Some(settled) = jobs.finish(id, stored) else {
        log::warn!("sheet {id} came after its request ended");
        if let Some(folder) = folder {
            let _ = std::fs::remove_dir_all(folder);
        }
        return (Err(format!("sheet request {id} is not in flight")), None);
    };
    if let Some(err) = &error {
        log::warn!("sheet {id} refused: {err}");
    }
    (error.map_or(Ok(()), Err), Some(settled))
}

fn prewarm_if_due(app: &AppHandle) {
    if app.state::<AppState>().sheets.prewarm_due() {
        request_prewarm(app);
    }
}

fn after_finish(app: &AppHandle, settled: Settled) {
    for dir in settled.evicted {
        crate::actions::delete_later(app, dir);
    }
    if settled.prewarm {
        request_prewarm(app);
    }
}

fn send(app: &AppHandle, request: &SheetRequest) {
    // A strip that is still loading picks the request up with `sheet_pending`.
    if app.get_webview_window(STRIP).is_none() {
        crate::windows_ui::sync_strip(app);
    }
    if let Err(err) = app.emit_to(STRIP, REQUEST_EVENT, request) {
        log::warn!("could not send sheet request {}: {err}", request.request_id);
    }
}

/// The snapshots of the sheet and its key. Without text comments in these snapshots, the sheet with and the
/// one without comments are the same picture, so they share a key.
fn select(
    session: &Session,
    letters: Option<&[char]>,
    with_comments: bool,
    title: &str,
) -> Result<(Key, Vec<Snapshot>), String> {
    let snapshots: Vec<Snapshot> = match letters {
        None => session.snapshots.clone(),
        Some(letters) => letters
            .iter()
            .map(|&l| session.get(l).cloned().ok_or_else(|| format!("snapshot {l} is no longer in the set")))
            .collect::<Result<_, _>>()?,
    };
    if snapshots.is_empty() {
        return Err("the set is empty".into());
    }
    let key = Key {
        session_id: session.id.clone(),
        items: snapshots.iter().map(|s| (s.letter, s.revision)).collect(),
        title: title.to_string(),
        with_comments: with_comments && snapshots.iter().any(Snapshot::has_text),
    };
    Ok((key, snapshots))
}

/// A PNG of exactly the announced size, within the hard caps. Only the header is read.
fn check_png(png: &[u8], meta: &SheetPutMeta) -> Result<(), String> {
    if png.len() < 24 || png[..8] != PNG_SIGNATURE || &png[12..16] != b"IHDR" {
        return Err("not a PNG".into());
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    if (width, height) != (meta.width, meta.height) {
        return Err(format!("the PNG is {width}x{height}, announced {}x{}", meta.width, meta.height));
    }
    if width == 0 || height == 0 || width.max(height) > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_AREA {
        return Err(format!("a {width}x{height} sheet is over the size caps"));
    }
    Ok(())
}

/// The set the request was made for is still current and still holds every letter; returns its folder.
fn still_current(session: Option<&Session>, session_id: &str, letters: &[char]) -> Result<PathBuf, String> {
    let session = session.filter(|s| s.id == session_id).ok_or("the set changed")?;
    match letters.iter().find(|&&l| session.get(l).is_none()) {
        Some(l) => Err(format!("snapshot {l} is no longer in the set")),
        None => Ok(session.dir.clone()),
    }
}

/// `<set dir>/sheets/<requestId>/Screenshots-A-F.png`: one folder per request, so a sheet that a paste may
/// still be reading is never overwritten. ASCII names, as some web uploaders mangle others.
fn sheet_path(set_dir: &Path, request_id: u64, letters: &[char]) -> PathBuf {
    set_dir.join("sheets").join(request_id.to_string()).join(format!("Screenshots-{}.png", letter_range(letters)))
}

fn store(set_dir: &Path, request_id: u64, letters: &[char], png: Vec<u8>, meta: &SheetPutMeta) -> Result<Sheet, String> {
    let path = sheet_path(set_dir, request_id, letters);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    session::write_file_atomic(&path, &png).map_err(|e| e.to_string())?;
    Ok(Sheet { png: Arc::new(png), path, width: meta.width, height: meta.height, cramped: meta.cramped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Mark;
    use std::time::SystemTime;

    const SHORT: Duration = Duration::from_millis(50);

    fn temp_session(name: &str) -> Session {
        let root = std::env::temp_dir().join(format!("wellsnap-test-sheet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Session::new(&root, SystemTime::now())
    }

    fn commented(text: &str) -> Vec<Mark> {
        vec![Mark { num: 1, comment: text.into() }]
    }

    fn key(session: &str, letters: &str) -> Key {
        Key {
            session_id: session.into(),
            items: letters.chars().map(|l| (l, 0)).collect(),
            title: "Screenshot".into(),
            with_comments: true,
        }
    }

    fn sheet(name: &str) -> Sheet {
        let path = PathBuf::from(format!("sheets/{name}/Screenshots-A.png"));
        Sheet { png: Arc::new(name.as_bytes().to_vec()), path, width: 10, height: 10, cramped: false }
    }

    fn meta(request_id: u64, width: u32, height: u32) -> SheetPutMeta {
        SheetPutMeta { request_id, width, height, cramped: false }
    }

    /// Signature plus an IHDR header; only the header is ever read.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend(13u32.to_be_bytes());
        png.extend(b"IHDR");
        png.extend(width.to_be_bytes());
        png.extend(height.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        png
    }

    /// Runs `obtain` on a thread and returns the request it sent.
    fn obtain_in_background(
        jobs: &Arc<SheetJobs>,
        key: Key,
        timeout: Duration,
    ) -> (std::thread::JoinHandle<Result<Sheet, String>>, std::sync::mpsc::Receiver<u64>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let jobs = Arc::clone(jobs);
        let handle = std::thread::spawn(move || jobs.obtain(key, vec![], timeout, |r| tx.send(r.request_id).unwrap()));
        (handle, rx)
    }

    #[test]
    fn request_and_put_meta_match_ipc() {
        let snapshot = Snapshot {
            letter: 'A',
            path: PathBuf::from("A.png"),
            width: 30,
            height: 20,
            marks: vec![],
            comments_on_image: false,
            revision: 0,
            scale: 1.5,
        };
        let request = SheetRequest {
            request_id: 7,
            session_id: "s1".into(),
            title: "Screenshot".into(),
            with_comments: true,
            snapshots: vec![snapshot],
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({
                "requestId": 7,
                "sessionId": "s1",
                "title": "Screenshot",
                "withComments": true,
                "snapshots": [{"letter": "A", "width": 30, "height": 20, "marks": [], "commentsOnImage": false, "revision": 0, "scale": 1.5}],
            })
        );
        // The strip sends JSON.stringify(meta), URI-encoded in the x-meta header.
        let header = urlencoding::encode(r#"{"requestId":7,"width":1920,"height":1080,"cramped":true}"#).into_owned();
        let meta: SheetPutMeta = serde_json::from_str(&urlencoding::decode(&header).unwrap()).unwrap();
        assert_eq!((meta.request_id, meta.width, meta.height, meta.cramped), (7, 1920, 1080, true));
    }

    #[test]
    fn a_put_must_be_a_png_of_the_announced_size_within_the_caps() {
        assert_eq!(check_png(&png_header(1920, 1080), &meta(1, 1920, 1080)), Ok(()));
        assert_eq!(check_png(&png_header(MAX_SIDE, 2929), &meta(1, MAX_SIDE, 2929)), Ok(()), "11.997 MP");
        let mut bad = png_header(10, 10);
        bad[1] = b'J';
        assert!(check_png(&bad, &meta(1, 10, 10)).is_err(), "signature");
        assert!(check_png(b"\x89PNG", &meta(1, 10, 10)).is_err(), "truncated");
        assert!(check_png(&png_header(1920, 1080), &meta(1, 1920, 1081)).unwrap_err().contains("announced"));
        for (w, h) in [(MAX_SIDE + 1, 10), (10, MAX_SIDE + 1), (4000, 3001), (0, 10)] {
            assert!(check_png(&png_header(w, h), &meta(1, w, h)).is_err(), "{w}x{h}");
        }
    }

    #[test]
    fn a_put_for_another_set_or_a_removed_letter_is_refused() {
        let mut s = temp_session("current");
        s.add(b"png", 10, 10, 1.0, vec![], false).unwrap();
        s.add(b"png", 10, 10, 1.0, vec![], false).unwrap();
        assert_eq!(still_current(Some(&s), &s.id, &['A', 'B']), Ok(s.dir.clone()));
        assert_eq!(still_current(Some(&s), "s0", &['A']), Err("the set changed".into()));
        assert_eq!(still_current(None, &s.id, &['A']), Err("the set changed".into()));
        s.remove('B');
        assert!(still_current(Some(&s), &s.id, &['A', 'B']).unwrap_err().contains("snapshot B"));
    }

    #[test]
    fn a_put_is_checked_stored_and_ends_its_request() {
        let mut s = temp_session("put");
        s.add(b"png", 10, 10, 1.0, vec![], false).unwrap();
        let k = Key { session_id: s.id.clone(), ..key("", "A") };
        let path = sheet_path(&s.dir, 2, &['A']);
        let session = Mutex::new(Some(s));
        let jobs = SheetJobs::default();
        let (result, settled) = take_put(&jobs, &session, &meta(1, 4, 2), png_header(4, 2));
        assert_eq!((result, settled), (Err("sheet request 1 is not in flight".into()), None));

        let id = jobs.lock().start(k.clone(), vec![]).request_id;
        let (result, settled) = take_put(&jobs, &session, &meta(id, 4, 3), png_header(4, 2));
        assert!(result.unwrap_err().contains("announced"));
        assert_eq!(settled, Some(Settled::default()), "a refused put ends the request");
        assert!(jobs.lock().failed.contains(&k));

        let id = jobs.lock().start(k.clone(), vec![]).request_id;
        let (result, settled) = take_put(&jobs, &session, &meta(id, 4, 2), png_header(4, 2));
        assert_eq!((result, settled), (Ok(()), Some(Settled::default())));
        assert_eq!(jobs.lock().cached(&k).unwrap().path, path);
        assert_eq!(std::fs::read(&path).unwrap(), png_header(4, 2));
        assert!(!jobs.lock().failed.contains(&k));

        let id = jobs.lock().start(Key { items: vec![('A', 0), ('B', 0)], ..k }, vec![]).request_id;
        let (result, _) = take_put(&jobs, &session, &meta(id, 4, 2), png_header(4, 2));
        assert!(result.unwrap_err().contains("snapshot B"), "a letter the set does not hold");
        session.lock().unwrap().take();
        let id = jobs.lock().start(key("other", "A"), vec![]).request_id;
        let (result, _) = take_put(&jobs, &session, &meta(id, 4, 2), png_header(4, 2));
        assert_eq!(result, Err("the set changed".into()));
    }

    #[test]
    fn a_sheet_stored_after_its_request_ended_is_refused_and_removed() {
        let s = temp_session("late");
        let stored = store(&s.dir, 9, &['A'], png_header(4, 2), &meta(9, 4, 2)).unwrap();
        let folder = stored.path.parent().unwrap().to_path_buf();
        assert!(folder.exists());
        let jobs = SheetJobs::default();
        let (result, settled) = conclude(&jobs, 9, Ok(stored));
        assert_eq!((result, settled), (Err("sheet request 9 is not in flight".into()), None));
        assert!(!folder.exists());
        assert!(jobs.lock().cache.is_empty());
    }

    #[test]
    fn a_prewarm_left_behind_by_a_timeout_is_due_once_the_slot_frees() {
        let jobs = SheetJobs::default();
        let with = key("s1", "AB");
        let without = Key { with_comments: false, ..with.clone() };
        jobs.prewarm(vec![(with, vec![]), (without, vec![])], SHORT, |_| ());
        assert!(!jobs.prewarm_due(), "the first sheet is still in flight");
        std::thread::sleep(SHORT);
        assert!(jobs.pending(SHORT).is_none(), "timed out");
        assert!(jobs.prewarm_due());
        assert!(!jobs.prewarm_due(), "taken once");
    }

    #[test]
    fn only_the_request_in_flight_is_accepted() {
        let jobs = SheetJobs::default();
        assert_eq!(jobs.inflight(1), None);
        jobs.prewarm(vec![(key("s1", "AB"), vec![])], SHEET_TIMEOUT, |_| ());
        assert_eq!(jobs.inflight(2), None, "a wrong id");
        assert_eq!(jobs.inflight(1), Some(("s1".to_string(), vec!['A', 'B'])));
        assert_eq!(jobs.finish(2, Ok(sheet("x"))), None);
        assert_eq!(jobs.finish(1, Ok(sheet("x"))), Some(Settled::default()));
        assert_eq!(jobs.finish(1, Ok(sheet("x"))), None, "answered twice");
    }

    #[test]
    fn sheets_are_named_after_their_letters() {
        let dir = Path::new("set");
        assert_eq!(sheet_path(dir, 7, &['A', 'B', 'F']), dir.join("sheets").join("7").join("Screenshots-A-F.png"));
        assert_eq!(sheet_path(dir, 8, &['C']), dir.join("sheets").join("8").join("Screenshots-C.png"));
        assert_eq!(letter_range(&[]), "");
        let s = temp_session("store");
        let stored = store(&s.dir, 3, &['A', 'B'], png_header(4, 2), &meta(3, 4, 2)).unwrap();
        assert!(stored.path.ends_with("sheets/3/Screenshots-A-B.png"));
        assert_eq!(std::fs::read(&stored.path).unwrap(), *stored.png);
        assert_eq!((stored.width, stored.height, stored.cramped), (4, 2, false));
    }

    #[test]
    fn selection_follows_the_letters_and_merges_identical_variants() {
        let mut s = temp_session("select");
        s.add(b"png", 10, 10, 1.0, commented("on image"), true).unwrap();
        s.add(b"png", 10, 10, 2.0, vec![], false).unwrap();
        let (with, snaps) = select(&s, None, true, "Снимок").unwrap();
        assert_eq!(snaps.len(), 2);
        assert_eq!(with.items, vec![('A', 0), ('B', 0)]);
        assert!(!with.with_comments, "no text comments: both variants are one picture");
        assert_eq!(select(&s, None, false, "Снимок").unwrap().0, with);
        assert_ne!(select(&s, None, true, "Screenshot").unwrap().0, with, "the language is in the key");

        s.add(b"png", 10, 10, 1.0, commented("as text"), false).unwrap();
        let (with, _) = select(&s, Some(&['C', 'A']), true, "Снимок").unwrap();
        assert!(with.with_comments);
        assert_eq!(with.items, vec![('C', 0), ('A', 0)]);
        assert_ne!(select(&s, Some(&['C', 'A']), false, "Снимок").unwrap().0, with);
        assert!(select(&s, Some(&['D']), true, "Снимок").unwrap_err().contains("snapshot D"));
        assert!(select(&s, Some(&[]), true, "Снимок").is_err());
    }

    #[test]
    fn a_waiter_wakes_when_its_sheet_arrives() {
        let jobs = Arc::new(SheetJobs::default());
        let (waiter, sent) = obtain_in_background(&jobs, key("s1", "A"), SHEET_TIMEOUT);
        let id = sent.recv().unwrap();
        assert!(jobs.pending(SHEET_TIMEOUT).is_some_and(|r| r.request_id == id), "the strip can pick it up");
        jobs.finish(id, Ok(sheet("a"))).unwrap();
        assert_eq!(*waiter.join().unwrap().unwrap().png, b"a");
        assert!(jobs.pending(SHEET_TIMEOUT).is_none());
        // Cached now: no new request.
        let again = jobs.obtain(key("s1", "A"), vec![], SHORT, |_| panic!("sent again")).unwrap();
        assert_eq!(*again.png, b"a");
    }

    #[test]
    fn a_failure_reaches_the_waiter() {
        let jobs = Arc::new(SheetJobs::default());
        let (waiter, sent) = obtain_in_background(&jobs, key("s1", "A"), SHEET_TIMEOUT);
        jobs.finish(sent.recv().unwrap(), Err("HTTP 404".into())).unwrap();
        assert_eq!(waiter.join().unwrap().unwrap_err(), "HTTP 404");
    }

    #[test]
    fn an_unanswered_request_times_out_and_frees_the_slot() {
        let jobs = SheetJobs::default();
        let started = Instant::now();
        let mut sent = 0;
        assert!(jobs.obtain(key("s1", "A"), vec![], SHORT, |_| sent += 1).unwrap_err().contains("in time"));
        assert!(started.elapsed() >= SHORT);
        assert_eq!(sent, 1);
        assert!(jobs.pending(SHORT).is_none(), "the slot is free");
        // A late answer is refused.
        assert_eq!(jobs.finish(1, Ok(sheet("late"))), None);
    }

    #[test]
    fn requests_take_turns_and_equal_ones_join() {
        let jobs = Arc::new(SheetJobs::default());
        let (first, first_sent) = obtain_in_background(&jobs, key("s1", "A"), SHEET_TIMEOUT);
        let first_id = first_sent.recv().unwrap();
        let (joined, joined_sent) = obtain_in_background(&jobs, key("s1", "A"), SHEET_TIMEOUT);
        let (other, other_sent) = obtain_in_background(&jobs, key("s1", "AB"), SHEET_TIMEOUT);
        std::thread::sleep(SHORT);
        assert!(other_sent.try_recv().is_err(), "waits for the slot");
        jobs.finish(first_id, Ok(sheet("a"))).unwrap();
        let other_id = other_sent.recv().unwrap();
        assert_eq!(other_id, first_id + 1);
        jobs.finish(other_id, Ok(sheet("ab"))).unwrap();
        assert_eq!(*first.join().unwrap().unwrap().png, b"a");
        assert_eq!(*joined.join().unwrap().unwrap().png, b"a");
        assert!(joined_sent.try_recv().is_err(), "joined without a request of its own");
        assert_eq!(*other.join().unwrap().unwrap().png, b"ab");
    }

    #[test]
    fn a_request_nobody_answers_gives_up_its_slot() {
        let jobs = SheetJobs::default();
        // A prewarm has no waiter of its own; the next caller frees its slot once it is older than the timeout.
        let started = Instant::now();
        jobs.prewarm(vec![(key("s1", "A"), vec![])], SHEET_TIMEOUT, |_| ());
        let mut sent = vec![];
        let result = jobs.obtain(key("s1", "AB"), vec![], SHORT, |r| sent.push(r.request_id));
        assert_eq!(sent, vec![2], "sent once the prewarm expired");
        assert!(result.unwrap_err().contains("in time"));
        assert!(started.elapsed() >= SHORT * 2, "waited for the prewarm, then for its own request");
    }

    #[test]
    fn prewarm_asks_once_and_continues_after_the_slot_frees() {
        let jobs = SheetJobs::default();
        let with = key("s1", "AB");
        let without = Key { with_comments: false, ..with.clone() };
        let wanted = || vec![(with.clone(), vec![]), (without.clone(), vec![])];
        let mut sent = vec![];
        jobs.prewarm(wanted(), SHEET_TIMEOUT, |r| sent.push(r.with_comments));
        jobs.prewarm(wanted(), SHEET_TIMEOUT, |r| sent.push(r.with_comments));
        assert_eq!(sent, vec![true], "one request at a time");
        let settled = jobs.finish(1, Ok(sheet("with"))).unwrap();
        assert!(settled.prewarm, "the variant without comments is still missing");
        jobs.prewarm(wanted(), SHEET_TIMEOUT, |r| sent.push(r.with_comments));
        assert_eq!(sent, vec![true, false]);
        // A failed sheet is not prewarmed again, so a broken strip cannot loop.
        assert!(!jobs.finish(2, Err("broken".into())).unwrap().prewarm);
        jobs.prewarm(wanted(), SHEET_TIMEOUT, |r| sent.push(r.with_comments));
        assert_eq!(sent, vec![true, false]);
        // An explicit request still retries it.
        let mut retried = false;
        let _ = jobs.obtain(without, vec![], SHORT, |_| retried = true);
        assert!(retried);
    }

    #[test]
    fn the_cache_keeps_eight_sheets_of_the_current_set() {
        let jobs = SheetJobs::default();
        let letters = ["A", "AB", "ABC", "ABCD", "B", "BC", "BCD", "C", "CD"];
        let mut evicted = vec![];
        for (i, l) in letters.iter().enumerate() {
            jobs.prewarm(vec![(key("s1", l), vec![])], SHEET_TIMEOUT, |_| ());
            let name = format!("{i}");
            evicted.extend(jobs.finish(i as u64 + 1, Ok(sheet(&name))).unwrap().evicted);
        }
        assert_eq!(evicted, vec![PathBuf::from("sheets/0")], "the oldest folder goes");
        assert!(jobs.obtain(key("s1", "A"), vec![], SHORT, |_| ()).is_err(), "evicted, so asked again");
        assert_eq!(*jobs.obtain(key("s1", "CD"), vec![], SHORT, |_| panic!()).unwrap().png, b"8");
        // Another set: everything cached for the old one is dropped.
        let mut sent = false;
        let _ = jobs.obtain(key("s2", "CD"), vec![], SHORT, |_| sent = true);
        assert!(sent);
        assert!(jobs.lock().cache.is_empty());
        jobs.lock().cache.push((key("s2", "A"), sheet("x")));
        jobs.clear_cache();
        assert!(jobs.lock().cache.is_empty());
    }
}
