//! The disk outbox. Every Sentry envelope, from the app and from the crash reporter process, is written here
//! synchronously by `DiskTransport` (safe inside the `panic = "abort"` hook) and nothing goes to the network
//! from inside the SDK; upload.rs sends the files later, when consent and onboarding allow it.
//!
//! File names: `<unix ms>-<uuid>.<f|e>.envelope`, `f` for fatal reports (crashes), `e` for other errors.
//! Rules: crash mode "never" writes nothing, "ask" keeps only fatal reports, "always" keeps everything; at most
//! 5 fatal reports, 30 files and 20 MB in total, nothing older than 30 days, the oldest errors go first.

use super::js::DEBUG_IDS_EXTRA;
use super::{SentItem, SentKind};
use crate::settings::CrashMode;
use sentry::protocol::{Envelope, Level, Value};
use sentry::Transport;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

const MAX_FATAL: usize = 5;
const MAX_FILES: usize = 30;
const MAX_BYTES: u64 = 20 * 1024 * 1024;
const MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const EXTENSION: &str = ".envelope";
/// Shown in "Show what was sent": the event JSON, cut here.
const DETAIL_MAX: usize = 16 * 1024;

/// The crash mode this process works with: read from settings.json at start, changed by Settings.
static MODE: AtomicU8 = AtomicU8::new(0);

pub fn set_mode(mode: CrashMode) {
    MODE.store(
        match mode {
            CrashMode::Always => 0,
            CrashMode::Ask => 1,
            CrashMode::Never => 2,
        },
        Ordering::SeqCst,
    );
}

pub fn mode() -> CrashMode {
    match MODE.load(Ordering::SeqCst) {
        0 => CrashMode::Always,
        1 => CrashMode::Ask,
        _ => CrashMode::Never,
    }
}

pub struct DiskTransport {
    pub dir: PathBuf,
}

impl Transport for DiskTransport {
    fn send_envelope(&self, envelope: Envelope) {
        let _quiet = super::crash::Quiet::enter();
        // Nothing to report from here: a log line would come back through the log target.
        let _ = accept(&self.dir, &envelope, mode(), now_ms());
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Writes the envelope if the mode keeps it; returns the file.
pub fn accept(dir: &Path, envelope: &Envelope, mode: CrashMode, now_ms: i64) -> std::io::Result<Option<PathBuf>> {
    // Only events: a client report or a session alone tells nothing worth a request.
    let Some(event) = envelope.event() else { return Ok(None) };
    let fatal = event.level == Level::Fatal;
    let keep = match mode {
        CrashMode::Always => true,
        CrashMode::Ask => fatal,
        CrashMode::Never => false,
    };
    if !keep {
        return Ok(None);
    }
    let bytes = serialize(envelope)?;
    std::fs::create_dir_all(dir)?;
    let name = format!("{now_ms:013}-{}.{}{EXTENSION}", uuid::Uuid::new_v4().simple(), if fatal { 'f' } else { 'e' });
    let path = dir.join(name);
    let tmp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    prune(dir, now_ms);
    Ok(Some(path))
}

/// The envelope as Sentry reads it. A JavaScript event is written as JSON, for two fields the typed protocol
/// lacks: its script debug ids (an extra here) become `debug_meta.images` of type "sourcemap"
/// (https://docs.sentry.io/platforms/javascript/sourcemaps/troubleshooting_js/debug-ids/), and
/// `sdk.settings.infer_ip` is "never": without it Relay's legacy rule fills in the sender's IP for platform
/// "javascript" events that have no `user.ip_address`.
fn serialize(envelope: &Envelope) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let Some(event) = envelope.event().filter(|e| e.platform == "javascript") else {
        envelope.to_writer(&mut out)?;
        return Ok(out);
    };
    let mut json = serde_json::to_value(event).map_err(std::io::Error::other)?;
    json["sdk"]["settings"]["infer_ip"] = Value::from("never");
    let ids = json.get_mut("extra").and_then(Value::as_object_mut).and_then(|extra| extra.remove(DEBUG_IDS_EXTRA));
    let images: Vec<Value> = ids
        .as_ref()
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(code_file, id)| serde_json::json!({"type": "sourcemap", "code_file": code_file, "debug_id": id}))
        .collect();
    if !images.is_empty() {
        json["debug_meta"] = serde_json::json!({ "images": images });
    }
    let payload = serde_json::to_vec(&json).map_err(std::io::Error::other)?;
    writeln!(out, "{}", serde_json::json!({ "event_id": event.event_id.to_string() }))?;
    writeln!(out, "{}", serde_json::json!({ "type": "event", "length": payload.len() }))?;
    out.extend_from_slice(&payload);
    out.push(b'\n');
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxFile {
    pub path: PathBuf,
    pub ms: i64,
    pub fatal: bool,
    pub size: u64,
}

/// Oldest first.
pub fn list(dir: &Path) -> Vec<OutboxFile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<OutboxFile> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let stem = name.strip_suffix(EXTENSION)?;
            let (rest, kind) = stem.rsplit_once('.')?;
            let ms = rest.split('-').next()?.parse().ok()?;
            let size = entry.metadata().map_or(0, |m| m.len());
            Some(OutboxFile { path: entry.path(), ms, fatal: kind == "f", size })
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// Which files go: too old, fatal beyond 5, then beyond 30 files or 20 MB (errors before crashes).
pub fn plan_prune(files: &[OutboxFile], now_ms: i64) -> Vec<usize> {
    let mut gone = vec![false; files.len()];
    for (i, f) in files.iter().enumerate() {
        gone[i] = now_ms - f.ms > MAX_AGE_MS;
    }
    let fatal: Vec<usize> = (0..files.len()).filter(|&i| files[i].fatal && !gone[i]).collect();
    for &i in fatal.iter().take(fatal.len().saturating_sub(MAX_FATAL)) {
        gone[i] = true;
    }
    let order: Vec<usize> =
        (0..files.len()).filter(|&i| !files[i].fatal).chain((0..files.len()).filter(|&i| files[i].fatal)).collect();
    for i in order {
        let kept: Vec<usize> = (0..files.len()).filter(|&k| !gone[k]).collect();
        let bytes: u64 = kept.iter().map(|&k| files[k].size).sum();
        if kept.len() <= MAX_FILES && bytes <= MAX_BYTES {
            break;
        }
        gone[i] = true;
    }
    (0..files.len()).filter(|&i| gone[i]).collect()
}

pub fn prune(dir: &Path, now_ms: i64) {
    let files = list(dir);
    for i in plan_prune(&files, now_ms) {
        let _ = std::fs::remove_file(&files[i].path);
    }
}

/// Deletes every waiting report (crash mode "never", Delete).
pub fn purge(dir: &Path) {
    remove_if(dir, |_| true);
}

pub fn remove_if(dir: &Path, gone: impl Fn(&OutboxFile) -> bool) {
    for file in list(dir).into_iter().filter(|f| gone(f)) {
        let _ = std::fs::remove_file(file.path);
    }
}

/// Whether the uploader sends this file now: everything in "always"; in "ask" only the crash reports the user
/// approved with "Send" (errors kept from an earlier "always" never go out on that answer).
pub fn sendable(file: &OutboxFile, mode: CrashMode, approved_until: i64) -> bool {
    match mode {
        CrashMode::Always => true,
        CrashMode::Ask => file.fatal && file.ms <= approved_until,
        CrashMode::Never => false,
    }
}

/// Crash reports the "ask" card asks about: not approved yet.
pub fn unanswered(file: &OutboxFile, approved_until: i64) -> bool {
    file.fatal && file.ms > approved_until
}

/// The newest crash report on disk, the mark a "Send" approves up to.
pub fn newest_fatal(dir: &Path) -> Option<i64> {
    list(dir).iter().filter(|f| f.fatal).map(|f| f.ms).max()
}

/// The history line for an uploaded file: its title, and the event JSON as it was sent.
pub fn describe(bytes: &[u8], at: i64) -> SentItem {
    let mut event: Option<serde_json::Value> = None;
    let mut attachments = Vec::new();
    let mut rest = bytes.splitn(2, |b| *b == b'\n').nth(1).unwrap_or_default();
    while !rest.is_empty() {
        let (header_line, after) = split_line(rest);
        let Ok(header) = serde_json::from_slice::<serde_json::Value>(header_line) else { break };
        let length = header["length"].as_u64().map(|l| l as usize);
        let (payload, after) = match length {
            Some(len) if len <= after.len() => {
                let (payload, tail) = after.split_at(len);
                (payload, tail.strip_prefix(b"\n").unwrap_or(tail))
            }
            _ => split_line(after),
        };
        match header["type"].as_str() {
            Some("event") => event = serde_json::from_slice(payload).ok(),
            Some("attachment") => {
                attachments.push((header["filename"].as_str().unwrap_or("").to_string(), payload.len()));
            }
            _ => {}
        }
        rest = after;
    }
    let event = event.unwrap_or_default();
    let fatal = event["level"].as_str() == Some("fatal");
    let summary = title(&event);
    let mut detail = serde_json::to_string_pretty(&event).unwrap_or_default();
    detail = super::scrub::truncate(&detail, DETAIL_MAX);
    for (name, len) in attachments {
        detail.push_str(&format!("\n\n+ {name} ({} KB)", len.div_ceil(1024)));
    }
    SentItem { at, kind: if fatal { SentKind::Crash } else { SentKind::Error }, summary, detail: Some(detail) }
}

fn split_line(bytes: &[u8]) -> (&[u8], &[u8]) {
    match bytes.iter().position(|b| *b == b'\n') {
        Some(at) => (&bytes[..at], &bytes[at + 1..]),
        None => (bytes, &[]),
    }
}

fn title(event: &serde_json::Value) -> String {
    let exception = &event["exception"]["values"][0];
    let text = if let Some(ty) = exception["type"].as_str() {
        match exception["value"].as_str() {
            Some(value) => format!("{ty}: {value}"),
            None => ty.to_string(),
        }
    } else if let Some(message) = event["logentry"]["message"].as_str().or(event["message"].as_str()) {
        message.to_string()
    } else if event["level"].as_str() == Some("fatal") {
        "crash".to_string()
    } else {
        "error".to_string()
    };
    super::scrub::truncate(&text, 120)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::{Attachment, AttachmentType, ClientSdkInfo, Event, Exception};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wellsnap-test-outbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn envelope(level: Level) -> Envelope {
        let event = Event {
            level,
            exception: vec![Exception { ty: "panic".into(), value: Some("boom".into()), ..Default::default() }].into(),
            ..Default::default()
        };
        Envelope::from(event)
    }

    #[test]
    fn round_trip_through_the_sdk_reader() {
        let dir = temp_dir("roundtrip");
        let mut env = envelope(Level::Fatal);
        env.add_item(Attachment {
            buffer: vec![1, 2, 3],
            filename: "x.dmp".into(),
            ty: Some(AttachmentType::Minidump),
            ..Default::default()
        });
        let path = accept(&dir, &env, CrashMode::Always, 1_000).unwrap().unwrap();
        assert!(path.file_name().unwrap().to_str().unwrap().ends_with(".f.envelope"));
        let read = Envelope::from_path(&path).unwrap();
        assert_eq!(read.event().unwrap().exception.values[0].value.as_deref(), Some("boom"));
        let item = describe(&std::fs::read(&path).unwrap(), 7);
        assert_eq!((item.kind, item.summary.as_str(), item.at), (SentKind::Crash, "panic: boom", 7));
        assert!(item.detail.unwrap().ends_with("+ x.dmp (1 KB)"));
        let files = list(&dir);
        assert_eq!((files.len(), files[0].fatal, files[0].ms), (1, true, 1_000));
        purge(&dir);
        assert!(list(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_crash_mode_decides_what_is_kept() {
        let dir = temp_dir("modes");
        let ask_error = accept(&dir, &envelope(Level::Error), CrashMode::Ask, 1).unwrap();
        assert!(ask_error.is_none(), "ask: errors are breadcrumbs");
        assert!(accept(&dir, &envelope(Level::Fatal), CrashMode::Ask, 2).unwrap().is_some(), "ask: crashes wait");
        assert!(accept(&dir, &envelope(Level::Fatal), CrashMode::Never, 3).unwrap().is_none());
        assert!(accept(&dir, &envelope(Level::Error), CrashMode::Always, 4).unwrap().is_some());
        assert!(accept(&dir, &Envelope::new(), CrashMode::Always, 5).unwrap().is_none(), "no event, no file");
        assert_eq!(list(&dir).iter().map(|f| f.fatal).collect::<Vec<_>>(), [true, false]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn javascript_events_carry_source_map_images() {
        let mut event = Event { level: Level::Error, platform: "javascript".into(), ..Default::default() };
        event.extra.insert(DEBUG_IDS_EXTRA.into(), serde_json::json!({"app:///assets/a.js": "4f7a-id"}));
        let bytes = serialize(&Envelope::from(event)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let header: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(header["length"].as_u64().unwrap() as usize, lines[2].len());
        let payload: serde_json::Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(
            payload["debug_meta"]["images"],
            serde_json::json!([{"type": "sourcemap", "code_file": "app:///assets/a.js", "debug_id": "4f7a-id"}])
        );
        assert!(payload["extra"].get(DEBUG_IDS_EXTRA).is_none());
        assert_eq!(payload["sdk"]["settings"]["infer_ip"], "never");
        assert_eq!(describe(&bytes, 0).kind, SentKind::Error);

        // Without debug ids: still no IP inference, and no empty sections.
        let sdk =
            ClientSdkInfo { name: "sentry.rust".into(), version: "0.49.3".into(), integrations: vec![], packages: vec![] };
        let bare = Event {
            level: Level::Error,
            platform: "javascript".into(),
            sdk: Some(std::borrow::Cow::Owned(sdk)),
            ..Default::default()
        };
        let bytes = serialize(&Envelope::from(bare)).unwrap();
        let read = Envelope::from_slice(&bytes).unwrap();
        assert_eq!(read.event().unwrap().sdk.as_ref().unwrap().name, "sentry.rust", "the SDK reader still parses it");
        let payload: serde_json::Value =
            serde_json::from_str(String::from_utf8(bytes).unwrap().lines().nth(2).unwrap()).unwrap();
        assert_eq!(payload["sdk"]["settings"]["infer_ip"], "never");
        assert!(payload.get("debug_meta").is_none() && payload.get("extra").is_none(), "{payload}");
    }

    #[test]
    fn ask_mode_sends_only_approved_crashes() {
        let file = |ms: i64, fatal: bool| OutboxFile { path: PathBuf::new(), ms, fatal, size: 1 };
        let (old_crash, old_error, new_crash) = (file(100, true), file(50, false), file(300, true));
        for f in [&old_crash, &old_error, &new_crash] {
            assert!(sendable(f, CrashMode::Always, 0));
            assert!(!sendable(f, CrashMode::Never, i64::MAX));
        }
        // "Send" approved everything up to 200: the crash written later waits for its own answer.
        assert!(sendable(&old_crash, CrashMode::Ask, 200));
        assert!(!sendable(&old_error, CrashMode::Ask, 200), "errors are never part of a crash answer");
        assert!(!sendable(&new_crash, CrashMode::Ask, 200));
        assert!(!unanswered(&old_crash, 200) && !unanswered(&old_error, 200) && unanswered(&new_crash, 200));

        let dir = temp_dir("approve");
        accept(&dir, &envelope(Level::Fatal), CrashMode::Always, 10).unwrap();
        accept(&dir, &envelope(Level::Error), CrashMode::Always, 20).unwrap();
        accept(&dir, &envelope(Level::Fatal), CrashMode::Always, 30).unwrap();
        assert_eq!(newest_fatal(&dir), Some(30));
        remove_if(&dir, |f| !f.fatal);
        assert_eq!(list(&dir).iter().map(|f| f.ms).collect::<Vec<_>>(), [10, 30]);
        remove_if(&dir, |f| unanswered(f, 10));
        assert_eq!(list(&dir).iter().map(|f| f.ms).collect::<Vec<_>>(), [10]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn caps_drop_old_files_and_errors_first() {
        let file =
            |ms: i64, fatal: bool, size: u64| OutboxFile { path: PathBuf::from(format!("{ms:013}")), ms, fatal, size };
        let now = MAX_AGE_MS + 100;
        // Too old.
        assert_eq!(plan_prune(&[file(50, true, 1), file(200, false, 1)], now), [0]);
        // Fatal beyond 5: the oldest go.
        let fatal: Vec<OutboxFile> = (0..7).map(|i| file(now - 10 + i, true, 1)).collect();
        assert_eq!(plan_prune(&fatal, now), [0, 1]);
        // 32 files: two errors go, crashes stay.
        let mut many: Vec<OutboxFile> = (0..2).map(|i| file(now - 100 + i, true, 1)).collect();
        many.extend((0..30).map(|i| file(now - 90 + i, false, 1)));
        assert_eq!(plan_prune(&many, now), [2, 3]);
        // 20 MB: the big error goes before the crash.
        let big = [file(now - 3, true, MAX_BYTES / 2), file(now - 2, false, MAX_BYTES), file(now - 1, false, 1)];
        assert_eq!(plan_prune(&big, now), [1]);
    }
}
