//! The Sentry client and the consent rules around it.
//!
//! Sources: Rust panics (sentry-panic), native crashes (minidumps written by the crash reporter process,
//! started unless the mode is "never"), `error!` lines from WellSnap's own code, JavaScript errors (js.rs) and
//! webview process failures. Info and higher lines from our code and warnings from other crates become
//! breadcrumbs, kept in one ring for the whole process (the SDK keeps one per thread) and copied to the crash
//! reporter so a native crash carries them too.
//!
//! Modes: "always" sends everything; "ask" keeps crashes on disk until the card on the next start is answered
//! and drops other errors; "never" captures nothing and empties the outbox. Release health is off: the
//! reporter sends a minidump without a session, so crash-free rates would look better than they are; the daily
//! `unclean_exits` counter covers it.

use super::outbox::{self, DiskTransport};
use super::paths::Dirs;
use super::scrub::Scrubber;
use super::{CrashAnswer, JsErrorReport, js, store, upload, usage};
use crate::settings::{CrashMode, Settings};
use crate::state::AppState;
use sentry::integrations::minidump::MinidumpIntegration;
use sentry::protocol::{Breadcrumb, Event, Level, User, Value};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const MAX_BREADCRUMBS: usize = 100;
/// The crash card waits for the app to settle after start.
const PROMPT_DELAY: Duration = Duration::from_secs(10);
/// A capture is running or the notice could not be shown: try again later, for about 20 minutes.
const PROMPT_RETRY: Duration = Duration::from_secs(30);
const PROMPT_ATTEMPTS: u32 = 40;
/// "Don't send" answers in a row that turn crash reports off.
const DONT_SEND_LIMIT: u32 = 3;

/// The client is set up: a sending build, not a plain debug run, and the folders are known.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CRUMBS: Mutex<VecDeque<Breadcrumb>> = Mutex::new(VecDeque::new());
static CRASH_ID: Mutex<Option<String>> = Mutex::new(None);
/// Tags on every event: os, arch, ui_lang, webview.
static TAGS: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// While held, log lines of this thread are not turned into breadcrumbs or reports (telemetry's own work).
pub struct Quiet(bool);

impl Quiet {
    pub fn enter() -> Quiet {
        Quiet(QUIET.with(|q| q.replace(true)))
    }
}

impl Drop for Quiet {
    fn drop(&mut self) {
        QUIET.with(|q| q.set(self.0));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

/// Before Tauri: reads the crash mode from settings.json and starts Sentry (and, unless the mode is "never",
/// the crash reporter process, which re-runs this binary and never returns from `sentry::init`). Debug builds
/// stay silent unless WELLSNAP_TELEMETRY_DEV=1.
pub fn early_init() -> Option<sentry::ClientInitGuard> {
    if !super::available() {
        return None;
    }
    if cfg!(debug_assertions) && std::env::var("WELLSNAP_TELEMETRY_DEV").as_deref() != Ok("1") {
        return None;
    }
    let dsn = option_env!("WELLSNAP_SENTRY_DSN")?.trim().parse().ok()?;
    let dirs = Dirs::detect()?;
    // Read without the side effects of settings::load (moving a corrupt file aside is the app's job).
    let settings: Settings =
        std::fs::read(dirs.settings()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    outbox::set_mode(settings.crash_reports);
    let state = store::init(dirs.clone());
    // Counters work from here on: the hotkeys are registered before `start`.
    usage::ON.store(settings.usage_stats, Ordering::Relaxed);
    *lock(&CRASH_ID) = state.crash_id;
    {
        let mut tags = lock(&TAGS);
        let os = if cfg!(target_os = "macos") { "macos" } else { "windows" };
        tags.insert("os".into(), os.into());
        tags.insert("arch".into(), usage::arch().into());
        let lang = crate::i18n::Lang::resolve(settings.language);
        if let Ok(Value::String(lang)) = serde_json::to_value(lang) {
            tags.insert("ui_lang".into(), lang);
        }
    }
    // A non-exhaustive struct: set field by field.
    let mut options = sentry::ClientOptions::new();
    options.dsn = Some(dsn);
    options.release = Some(format!("wellsnap@{}", env!("CARGO_PKG_VERSION")).into());
    options.environment = Some(if cfg!(debug_assertions) { "development" } else { "production" }.into());
    // An empty name keeps sentry-contexts from filling in the host name.
    options.server_name = Some("".into());
    options.send_default_pii = false;
    // Log errors get their stack in `on_log`; attached by the SDK it would also land on JavaScript events.
    options.attach_stacktrace = false;
    options.max_breadcrumbs = MAX_BREADCRUMBS;
    options.in_app_include = vec!["wellsnap_lib"];
    options.before_send = Some(Arc::new(before_send));
    options.before_breadcrumb = Some(Arc::new(|mut crumb| {
        Scrubber::global().breadcrumb(&mut crumb);
        Some(crumb)
    }));
    options.transport = Some(Arc::new(Arc::new(DiskTransport { dir: dirs.outbox() })));
    if settings.crash_reports != CrashMode::Never {
        options = options.add_integration(
            MinidumpIntegration::new().crashes_dir(dirs.dumps()).process_name("WellSnap Crash Reporter"),
        );
    }
    let guard = sentry::init(options);
    ACTIVE.store(true, Ordering::SeqCst);
    Some(guard)
}

fn dist() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos-universal"
    } else if cfg!(target_arch = "aarch64") {
        "windows-arm64"
    } else {
        "windows-x64"
    }
}

/// Runs for every event in both processes: consent, the random id, tags, the process-wide breadcrumbs and
/// scrubbing.
fn before_send(mut event: Event<'static>) -> Option<Event<'static>> {
    let keep = match outbox::mode() {
        CrashMode::Always => true,
        CrashMode::Ask => event.level == Level::Fatal,
        CrashMode::Never => false,
    };
    if !keep {
        return None;
    }
    event.user = Some(User { id: lock(&CRASH_ID).clone(), ..Default::default() });
    event.dist = Some(Cow::Borrowed(dist()));
    // The native module list means nothing for a script error; its source map images come from the outbox.
    if event.platform == "javascript" {
        event.debug_meta = Cow::Owned(Default::default());
    }
    for (key, value) in lock(&TAGS).iter() {
        event.tags.entry(key.clone()).or_insert_with(|| value.clone());
    }
    // In the crash reporter the ring is empty and the breadcrumbs copied over IPC are on the event already.
    let crumbs = lock(&CRUMBS);
    if !crumbs.is_empty() {
        event.breadcrumbs = crumbs.iter().cloned().collect::<Vec<_>>().into();
    }
    drop(crumbs);
    Scrubber::global().event(&mut event);
    Some(event)
}

fn add_breadcrumb(crumb: Breadcrumb) {
    {
        let mut crumbs = lock(&CRUMBS);
        if crumbs.len() == MAX_BREADCRUMBS {
            crumbs.pop_front();
        }
        crumbs.push_back(crumb.clone());
    }
    sentry::with_integration(|minidump: &MinidumpIntegration, _| minidump.add_breadcrumb(crumb));
}

/// Non-fatal reports exist only in "always" mode.
fn wants_errors() -> bool {
    active() && outbox::mode() == CrashMode::Always
}

/// The client-side limits: each fingerprint once a day per version, 30 a day in total.
fn allow(fingerprint: &str) -> bool {
    let key = format!("{}|{fingerprint}", env!("CARGO_PKG_VERSION"));
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let now = chrono::Utc::now().timestamp();
    store::update_quiet(|s| s.limits.allow(&key, &today, now)).unwrap_or(false)
}

/// The log plugin's prefix ("[date][time][target][LEVEL] ") is already on the message.
fn strip_log_prefix<'a>(formatted: &'a str, target: &str, level: log::Level) -> &'a str {
    let marker = format!("[{target}][{level}] ");
    formatted.find(&marker).map_or(formatted, |at| &formatted[at + marker.len()..])
}

fn level_of(level: log::Level) -> Level {
    match level {
        log::Level::Error => Level::Error,
        log::Level::Warn => Level::Warning,
        log::Level::Info => Level::Info,
        log::Level::Debug | log::Level::Trace => Level::Debug,
    }
}

/// Which log lines matter: info and higher from WellSnap, warnings and errors from libraries.
fn wanted(target: &str, level: log::Level) -> bool {
    let ours = target.starts_with("wellsnap");
    if ours { level <= log::Level::Info } else { level <= log::Level::Warn }
}

/// The fern target (see `log_dispatch`).
pub fn on_log(record: &log::Record) {
    if !active() || QUIET.with(Cell::get) || !wanted(record.target(), record.level()) {
        return;
    }
    let _quiet = Quiet::enter();
    let formatted = record.args().to_string();
    let message = strip_log_prefix(&formatted, record.target(), record.level());
    // WebView2 asks for a favicon on every page; Tauri logs the miss as an error.
    if message.contains("favicon.ico") {
        return;
    }
    let scrubber = Scrubber::global();
    add_breadcrumb(Breadcrumb {
        category: Some(record.target().to_string()),
        level: level_of(record.level()),
        message: Some(scrubber.text(message)),
        ..Default::default()
    });
    if record.level() != log::Level::Error || !record.target().starts_with("wellsnap") {
        return;
    }
    usage::add("errors_logged", 1);
    let file = record.file().unwrap_or("?").replace('\\', "/");
    let line = record.line().unwrap_or(0);
    if wants_errors() && allow(&format!("log|{file}|{line}")) {
        sentry::capture_event(Event {
            level: Level::Error,
            logger: Some(record.target().to_string()),
            message: Some(message.to_string()),
            stacktrace: sentry::integrations::backtrace::current_stacktrace(),
            fingerprint: Cow::Owned(vec!["log".into(), file.into(), line.to_string().into()]),
            ..Default::default()
        });
    }
}

pub fn report_js_error(page: &str, report: JsErrorReport) {
    if !active() {
        return;
    }
    let _quiet = Quiet::enter();
    usage::add("js_errors", 1);
    let (event, fingerprint) = js::event(page, &report, Scrubber::global());
    let message = event.exception.values.first().map(|e| format!("{}: {}", e.ty, e.value.as_deref().unwrap_or("")));
    add_breadcrumb(Breadcrumb {
        category: Some(format!("js.{}", js::page_name(page))),
        level: Level::Error,
        message,
        ..Default::default()
    });
    if wants_errors() && allow(&fingerprint) {
        sentry::capture_event(event);
    }
}

/// A webview's content process ended (WebView2 ProcessFailed, macOS WebContent terminated). The window stays,
/// so it is an error, not a crash of WellSnap: sent in "always" mode, a breadcrumb otherwise.
pub fn webview_failed(label: &str, kind: &str, reason: Option<&str>, exit_code: Option<i32>) {
    if !active() {
        return;
    }
    let _quiet = Quiet::enter();
    let page = js::page_name(label).to_string();
    let reason = reason.unwrap_or("unknown");
    add_breadcrumb(Breadcrumb {
        category: Some("webview".into()),
        level: Level::Error,
        message: Some(format!("{page}: {kind} ({reason})")),
        ..Default::default()
    });
    if !wants_errors() || !allow(&format!("webview|{kind}|{reason}")) {
        return;
    }
    let mut event = Event {
        level: Level::Error,
        message: Some(format!("WebView process failed: {kind} ({reason})")),
        fingerprint: Cow::Owned(vec!["webview".into(), kind.to_string().into(), reason.to_string().into()]),
        ..Default::default()
    };
    event.tags.insert("page".into(), page);
    event.tags.insert("webview_failure".into(), kind.to_string());
    if let Some(code) = exit_code {
        event.extra.insert("exit_code".into(), Value::from(code));
    }
    // The handler runs on the UI thread; writing the report is not its business.
    std::thread::spawn(move || {
        sentry::capture_event(event);
    });
}

pub fn pending_reports() -> u32 {
    store::dirs().map_or(0, |dirs| outbox::list(&dirs.outbox()).len() as u32)
}

fn os_version() -> String {
    match sentry::integrations::contexts::utils::os_context() {
        Some(sentry::protocol::Context::Os(os)) => {
            os.version.as_deref().map(usage::os_version_label).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// In `setup`: session bookkeeping, one-off events, the webview watch, the uploader and the crash card.
pub fn start(app: &AppHandle) {
    if !active() {
        return;
    }
    let Some(dirs) = store::dirs() else { return };
    if app.path().app_config_dir().ok().as_deref() != Some(dirs.config.as_path()) {
        log::warn!("telemetry config folder differs from Tauri's; using {}", dirs.config.display());
    }
    let settings = app.state::<AppState>().settings();
    outbox::set_mode(settings.crash_reports);
    usage::ON.store(settings.usage_stats, Ordering::Relaxed);
    if settings.crash_reports == CrashMode::Never {
        outbox::purge(&dirs.outbox());
    }

    let version = app.package_info().version.to_string();
    let (unclean, updated_from) = store::update(|s| {
        // A new version means the installer ended the previous run.
        let updated_from = s.last_version.clone().filter(|v| *v != version);
        let unclean = s.session_open && updated_from.is_none();
        s.session_open = true;
        s.last_version = Some(version.clone());
        if settings.usage_stats {
            if s.usage_id.is_none() {
                s.usage_id = Some(store::new_id());
                usage::queue(&mut s.usage, "stats_enabled", props([("source", "default".into())]));
            }
            if let Some(from) = &updated_from {
                usage::queue(&mut s.usage, "app_updated", props([("from_version", from.clone().into())]));
            }
        }
        (unclean, updated_from)
    })
    .unwrap_or_default();
    if unclean {
        usage::add("unclean_exits", 1);
    }
    usage::add("launches", 1);
    if let Some(from) = updated_from {
        log::info!("telemetry: first start after the update from {from}");
    }

    // Installs that finished onboarding before telemetry existed see the switches once, before anything goes.
    let seen = store::get().is_some_and(|s| s.consent_seen);
    if settings.onboarding_done && !seen {
        store::update(|s| s.consent_seen = true);
        crate::windows_ui::open_settings(app, Some("privacy"));
    }

    let webview_major = tauri::webview_version().ok().and_then(|v| v.split('.').next().map(str::to_string));
    if let Some(major) = &webview_major {
        lock(&TAGS).insert("webview".into(), major.clone());
        sentry::with_integration(|minidump: &MinidumpIntegration, _| {
            minidump.set_tag("webview".into(), Some(major.clone()));
        });
    }
    #[cfg(windows)]
    watch_webviews(app);

    upload::start(app.clone(), usage::Common { app_version: version, os_version: os_version(), webview_major });
    start_crash_prompt(app);
}

fn props<const N: usize>(pairs: [(&str, Value); N]) -> serde_json::Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// ProcessFailed on every webview: the ones that exist now, and through a plugin hook every later one
/// (overlays and the notice are rebuilt after idle release).
#[cfg(windows)]
fn watch_webviews(app: &AppHandle) {
    fn watch(webview: &tauri::Webview) {
        let label = webview.label().to_string();
        crate::webview_win::watch_process_failed(webview, move |failure| {
            webview_failed(&label, failure.kind, Some(failure.reason), failure.exit_code);
        });
    }
    let plugin = tauri::plugin::Builder::<tauri::Wry>::new("wellsnap-telemetry")
        .on_webview_ready(|webview| watch(&webview))
        .build();
    if let Err(err) = app.plugin(plugin) {
        log::warn!("webview crash watch not registered: {err}");
    }
    for window in app.webview_windows().values() {
        watch(window.as_ref());
    }
}

/// "ask" mode with crash reports waiting: the card, about 10 s after start, never over a capture.
fn start_crash_prompt(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PROMPT_DELAY);
        for _ in 0..PROMPT_ATTEMPTS {
            let settings = app.state::<AppState>().settings();
            let Some(state) = store::get() else { return };
            let Some(dirs) = store::dirs() else { return };
            let waiting = outbox::list(&dirs.outbox()).iter().filter(|f| f.fatal).count() as u32;
            if settings.crash_reports != CrashMode::Ask
                || !settings.onboarding_done
                || !state.consent_seen
                || state.dont_send_streak >= DONT_SEND_LIMIT
                || waiting == 0
            {
                return;
            }
            if crate::notice::show_crash_prompt(&app, waiting) {
                return;
            }
            std::thread::sleep(PROMPT_RETRY);
        }
    });
}

pub fn shutdown() {
    if active() {
        store::update(|s| s.session_open = false);
    }
}

pub fn settings_changed(app: &AppHandle, before: &Settings, after: &Settings) {
    if !active() {
        return;
    }
    outbox::set_mode(after.crash_reports);
    if before.crash_reports != after.crash_reports {
        match after.crash_reports {
            CrashMode::Never => {
                if let Some(dirs) = store::dirs() {
                    outbox::purge(&dirs.outbox());
                }
            }
            CrashMode::Always => upload::wake(),
            CrashMode::Ask => {}
        }
    }
    usage::ON.store(after.usage_stats, Ordering::Relaxed);
    if before.usage_stats != after.usage_stats {
        let source = if app.get_webview_window("onboarding").is_some() { "onboarding" } else { "settings" };
        store::update(|s| usage_switched(s, after.usage_stats, source));
        upload::wake();
    }
}

/// Statistics turned off: the id, today's counters and everything not sent yet are gone. Turned on: a new id
/// and a `stats_enabled` event.
fn usage_switched(s: &mut store::Store, on: bool, source: &str) {
    if on {
        s.usage_id.get_or_insert_with(store::new_id);
        usage::queue(&mut s.usage, "stats_enabled", props([("source", source.into())]));
    } else {
        s.usage_id = None;
        s.usage = Default::default();
        s.usage_retry = Default::default();
    }
}

pub fn onboarding_finished(app: &AppHandle) {
    if !active() {
        return;
    }
    let settings = app.state::<AppState>().settings();
    let mut properties = props([
        ("autostart", settings.autostart.into()),
        ("print_screen_taken_over", settings.print_screen_taken_over.into()),
    ]);
    if cfg!(target_os = "macos") {
        properties.insert("screen_permission".into(), crate::capture::has_permission().into());
        properties.insert("input_permission".into(), crate::paste::can_send_input().into());
    }
    store::update(|s| {
        s.consent_seen = true;
        if !s.onboarding_event_sent {
            s.onboarding_event_sent = true;
            if settings.usage_stats && s.usage_id.is_some() {
                usage::queue(&mut s.usage, "onboarding_finished", properties);
            }
        }
    });
    upload::wake();
}

pub fn crash_prompt_answer(app: &AppHandle, answer: CrashAnswer) {
    if !active() {
        return;
    }
    let purge = || {
        if let Some(dirs) = store::dirs() {
            outbox::purge(&dirs.outbox());
        }
    };
    match answer {
        CrashAnswer::Send => {
            store::update(|s| s.dont_send_streak = 0);
            upload::request_send();
        }
        CrashAnswer::DontSend => {
            purge();
            let streak = store::update(|s| {
                s.dont_send_streak += 1;
                s.dont_send_streak
            });
            if streak.is_some_and(|n| n >= DONT_SEND_LIMIT) {
                log::info!("crash reports turned off after {DONT_SEND_LIMIT} \"don't send\" answers");
                set_crash_mode(app, CrashMode::Never);
            }
        }
        CrashAnswer::Always => {
            store::update(|s| s.dont_send_streak = 0);
            set_crash_mode(app, CrashMode::Always);
        }
        CrashAnswer::Delete => purge(),
    }
}

/// Changes the setting as the Privacy tab would, and tells the webviews.
fn set_crash_mode(app: &AppHandle, mode: CrashMode) {
    let state = app.state::<AppState>();
    let before = state.settings();
    let after = state.update_settings(|s| s.crash_reports = mode);
    settings_changed(app, &before, &after);
    let _ = app.emit("settings:changed", &after);
}

pub fn reset_ids() {
    if !active() {
        return;
    }
    let crash_id = store::new_id();
    store::update(|s| {
        s.crash_id = Some(crash_id.clone());
        if s.usage_id.is_some() {
            s.usage_id = Some(store::new_id());
        }
    });
    *lock(&CRASH_ID) = Some(crash_id.clone());
    sentry::with_integration(|minidump: &MinidumpIntegration, _| {
        minidump.set_user(Some(User { id: Some(crash_id), ..Default::default() }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_lose_the_plugin_prefix() {
        let line = "[2026-10-22][18:33:01][wellsnap_lib::actions][ERROR] Copy failed: busy";
        assert_eq!(strip_log_prefix(line, "wellsnap_lib::actions", log::Level::Error), "Copy failed: busy");
        assert_eq!(strip_log_prefix("plain", "x", log::Level::Info), "plain");
    }

    #[test]
    fn our_info_and_library_warnings_become_breadcrumbs() {
        assert!(wanted("wellsnap_lib::overlay", log::Level::Info));
        assert!(!wanted("wellsnap_lib::overlay", log::Level::Debug));
        assert!(wanted("tauri::manager", log::Level::Warn));
        assert!(!wanted("tauri::manager", log::Level::Info));
        assert_eq!(level_of(log::Level::Warn), Level::Warning);
    }

    #[test]
    fn before_send_follows_the_mode_and_drops_the_host() {
        let crash = || Event { level: Level::Fatal, server_name: Some("DESKTOP-1".into()), ..Default::default() };
        let error = || Event { level: Level::Error, ..Default::default() };
        outbox::set_mode(CrashMode::Ask);
        assert!(before_send(error()).is_none(), "ask: errors stay breadcrumbs");
        let kept = before_send(crash()).unwrap();
        assert_eq!(kept.server_name, None);
        assert_eq!(kept.dist.as_deref(), Some(dist()));
        assert!(kept.user.unwrap().ip_address.is_none());
        outbox::set_mode(CrashMode::Never);
        assert!(before_send(crash()).is_none());
        outbox::set_mode(CrashMode::Always);
        assert!(before_send(error()).is_some());

        // The whole path of a captured event: client, before_send, the disk transport.
        let dir = std::env::temp_dir().join(format!("wellsnap-test-crash-client-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut options = sentry::ClientOptions::new();
        options.dsn = Some("https://k@o1.ingest.de.sentry.io/2".parse().unwrap());
        options.server_name = Some("DESKTOP-1".into());
        options.before_send = Some(Arc::new(before_send));
        options.transport = Some(Arc::new(Arc::new(DiskTransport { dir: dir.clone() })));
        let client = sentry::Client::with_options(options);
        let home = std::env::temp_dir().join("Users").join("someone").join("shot.png");
        client.capture_event(Event { message: Some(home.display().to_string()), ..error() }, None);
        let files = outbox::list(&dir);
        assert_eq!(files.len(), 1);
        let event = sentry::protocol::Envelope::from_path(&files[0].path).unwrap().event().cloned().unwrap();
        assert_eq!(event.server_name, None);
        let message = event.message.unwrap();
        assert!(message.ends_with("<user>/<file>.png") || message.ends_with(r"<user>\<file>.png"), "{message}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn statistics_off_forgets_the_id_and_everything_pending() {
        let mut s = store::Store::default();
        usage_switched(&mut s, true, "settings");
        let id = s.usage_id.clone().unwrap();
        assert_eq!(s.usage.pending[0].event, "stats_enabled");
        assert_eq!(s.usage.pending[0].properties["source"], "settings");
        s.usage.counts.insert("launches".into(), 3);
        s.usage_retry.back_off(0);
        usage_switched(&mut s, false, "settings");
        assert_eq!(s.usage_id, None);
        assert_eq!(s.usage, Default::default());
        assert_eq!(s.usage_retry, Default::default());
        usage_switched(&mut s, true, "onboarding");
        assert_ne!(s.usage_id.as_deref(), Some(id.as_str()), "a new id, not the old one");
    }

    #[test]
    fn quiet_nests_and_restores() {
        assert!(!QUIET.with(Cell::get));
        {
            let _outer = Quiet::enter();
            {
                let _inner = Quiet::enter();
            }
            assert!(QUIET.with(Cell::get), "the inner guard restores the outer state");
        }
        assert!(!QUIET.with(Cell::get));
    }
}
