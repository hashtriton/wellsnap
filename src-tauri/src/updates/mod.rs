//! Updates through GitHub Releases: the release workflow publishes latest.json signed with the updater key,
//! whose public half is in tauri.conf.json. In release builds a background thread checks shortly after start
//! and then every 12 hours. A new version waits 24 h plus this install's random 0-23 h, so a bad release can
//! still be pulled, then installs by itself while the computer is idle and WellSnap unused (policy.rs has the
//! rules). The user can always install it at once from the tray or About. After the restart a quiet
//! "updated" pill appears near the tray once the user is back.

mod policy;
mod store;
mod system;

use crate::state::AppState;
use policy::{Block, GateInput, GateMode, StartupOutcome};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(90);
const TICK: Duration = Duration::from_secs(60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Quit waits for an update at most this long; a slower download is left for the next run.
const QUIT_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const QUIT_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
const NOTICE_POLL: Duration = Duration::from_secs(2);
#[cfg(windows)]
const INSTALLER_DIR_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

static STARTED: OnceLock<Instant> = OnceLock::new();
/// Launched by the login item (`--autostart`, see lib.rs and autostart.rs).
static LOGIN_LAUNCH: AtomicBool = AtomicBool::new(false);
/// One install at a time, whichever path started it.
static INSTALLING: AtomicBool = AtomicBool::new(false);
/// Quit was chosen and waits for an update to go in; a second Quit does nothing.
static QUITTING: AtomicBool = AtomicBool::new(false);
static INSTALLED_COPY: OnceLock<bool> = OnceLock::new();
static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime::new());

/// Why "Update and restart" did nothing; About and the tray toast show it in the UI language.
pub const REFUSED_BUSY: &str = "busy";
pub const REFUSED_GONE: &str = "gone";

/// Kept in memory only: the package is 3 MB and a fresh endpoint check precedes every install anyway.
struct Runtime {
    /// Verified package of a version, kept until it is installed or another version is offered.
    package: Option<(String, Vec<u8>)>,
    /// The version whose download failed `download_failures` times in a row.
    backoff_version: Option<String>,
    download_failures: u32,
    next_download_at: Option<Instant>,
    /// The endpoint was unreachable: no request before this, except when the user asks.
    check_retry_at: Option<Instant>,
}

impl Runtime {
    const fn new() -> Self {
        Self { package: None, backoff_version: None, download_failures: 0, next_download_at: None, check_retry_at: None }
    }

    /// A check result: the package and the download backoff belong to one version and go when another one
    /// (or none) is offered.
    fn offered(&mut self, version: Option<&str>) {
        if self.package.as_ref().is_some_and(|(v, _)| Some(v.as_str()) != version) {
            self.package = None;
        }
        if self.backoff_version.is_some() && self.backoff_version.as_deref() != version {
            self.backoff_version = None;
            self.download_failures = 0;
            self.next_download_at = None;
        }
    }

    fn fetched(&mut self, ok: bool, now: Instant) {
        self.check_retry_at = if ok { None } else { Some(now + policy::CHECK_RETRY) };
    }

    fn check_waits(&self, now: Instant) -> bool {
        self.check_retry_at.is_some_and(|t| now < t)
    }

    fn downloaded(&mut self, version: &str, bytes: Vec<u8>) {
        self.package = Some((version.to_string(), bytes));
        self.backoff_version = None;
        self.download_failures = 0;
        self.next_download_at = None;
    }

    fn download_failed(&mut self, version: &str, now: Instant) {
        if self.backoff_version.as_deref() != Some(version) {
            self.backoff_version = Some(version.to_string());
            self.download_failures = 0;
        }
        self.download_failures += 1;
        self.next_download_at = Some(now + policy::backoff(self.download_failures));
    }

    fn download_waits(&self, version: &str, now: Instant) -> bool {
        self.backoff_version.as_deref() == Some(version) && self.next_download_at.is_some_and(|t| now < t)
    }

    /// The background install of `version` would only repeat a request that just failed: every attempt starts
    /// with a fresh check, and without the package in memory also needs a download.
    fn install_waits(&self, version: &str, now: Instant) -> bool {
        let has_package = self.package.as_ref().is_some_and(|(v, _)| v == version);
        self.check_waits(now) || (!has_package && self.download_waits(version, now))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum UpdatePhase {
    #[default]
    Idle,
    /// `auto_after`: when it may install by itself (Unix seconds).
    Available { info: UpdateInfo, auto_after: i64 },
    Installing { info: UpdateInfo },
}

impl UpdatePhase {
    /// The version the tray offers to install.
    pub fn offered(&self) -> Option<&UpdateInfo> {
        match self {
            UpdatePhase::Available { info, .. } => Some(info),
            _ => None,
        }
    }
}

/// What About shows; also sent as "update:status" whenever it changes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// "idle", "available" or "installing".
    phase: &'static str,
    info: Option<UpdateInfo>,
    /// The available version may install by itself once the settings allow it: an installed copy, not
    /// rejected, fewer than three failed attempts.
    auto_eligible: bool,
    /// The version installed in the last 14 days, for "Updated from" and "What's new".
    recent: Option<RecentUpdate>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentUpdate {
    from: Option<String>,
    version: String,
    /// None also once the user hid "What's new".
    notes: Option<String>,
    /// Unix seconds.
    at: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct GateCtx {
    idle_before: bool,
    resumed_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct Timeouts {
    check: Duration,
    download: Duration,
}

const NORMAL: Timeouts = Timeouts { check: CHECK_TIMEOUT, download: DOWNLOAD_TIMEOUT };

/// Startup bookkeeping (every build) and, in release builds, the background thread.
pub fn start(app: &AppHandle) {
    let _ = STARTED.set(Instant::now());
    LOGIN_LAUNCH.store(std::env::args().any(|a| a == "--autostart"), Ordering::SeqCst);
    // A dev build shares the config dir with the installed app; its version marker would hide the next real
    // update's notice there.
    let file = if cfg!(debug_assertions) { "update-state.dev.json" } else { "update-state.json" };
    match app.path().app_config_dir() {
        Ok(dir) => {
            store::init(dir.join(file));
        }
        Err(err) => log::error!("no config dir for the update state: {err}"),
    }
    let current = app.package_info().version.to_string();
    let onboarding_done = app.state::<AppState>().settings().onboarding_done;
    let before = store::get();
    let attempt_to = before.attempt.as_ref().map(|a| a.to.as_str());
    let outcome = policy::startup_outcome(before.last_run_version.as_deref(), &current, attempt_to, onboarding_done);
    if outcome != StartupOutcome::Same {
        log::info!("start of {current}: {outcome:?}");
    }
    let (after, ()) = store::update(|s| s.record_startup(&outcome, &current, now_secs()));
    if after.last_update.as_ref().is_some_and(|u| u.notice_pending) {
        start_notice_watcher(app);
    }
    #[cfg(windows)]
    {
        let name = app.package_info().name.clone();
        std::thread::spawn(move || system::remove_old_installer_dirs(&name, INSTALLER_DIR_MAX_AGE));
    }
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || run_loop(&app));
}

fn run_loop(app: &AppHandle) {
    std::thread::sleep(FIRST_CHECK_DELAY);
    let mut last_wall = now_secs();
    let mut resumed_at = None;
    let mut idle_before = false;
    let mut checked_once = false;
    loop {
        let now = now_secs();
        if policy::woke_up(last_wall, now) {
            log::info!("woke from sleep, no update install for a moment");
            resumed_at = Some(Instant::now());
        }
        let ctx = GateCtx { idle_before, resumed_at };
        idle_before = system::idle().is_some_and(|d| d >= policy::IDLE_FOR);
        // Read every time, so turning a setting off or on applies without a restart.
        let settings = app.state::<AppState>().settings();
        if settings.auto_check_updates {
            let due = !checked_once || policy::check_due(now, store::get().last_check_at, policy::CHECK_INTERVAL);
            // Offline or GitHub unreachable: `fetch` makes every background request wait an hour.
            if due && !RUNTIME.lock().unwrap().check_waits(Instant::now()) {
                checked_once = true;
                if let Err(err) = tauri::async_runtime::block_on(check(app)) {
                    log::warn!("update check failed: {err}");
                }
            }
            if settings.auto_install_updates {
                maybe_install(app, ctx);
            }
        }
        // From the end of this tick: a long check or download is not a sleep.
        last_wall = now_secs();
        std::thread::sleep(TICK);
    }
}

fn maybe_install(app: &AppHandle, ctx: GateCtx) {
    let offered = match &*app.state::<AppState>().update.lock().unwrap() {
        UpdatePhase::Available { info, auto_after } => Some((info.version.clone(), *auto_after)),
        _ => None,
    };
    let Some((version, auto_after)) = offered else { return };
    let now = now_secs();
    if now < auto_after
        || !may_retry(&store::get(), &version, now)
        || RUNTIME.lock().unwrap().install_waits(&version, Instant::now())
    {
        return;
    }
    let mode = if in_login_window(app) { GateMode::Login } else { GateMode::Auto };
    if let Err(block) = policy::install_gate(&gate_input(app, mode, ctx, &version)) {
        log::debug!("update {version} waits: {block:?}");
        return;
    }
    let result = tauri::async_runtime::block_on(install_with(app, mode, Some(&version), ctx, NORMAL, true));
    if let Err(err) = result {
        log::warn!("automatic update to {version} failed: {err}");
    }
}

/// Not rejected for a bad signature, and the last failed attempt is old enough.
fn may_retry(store: &store::UpdateStore, version: &str, now: i64) -> bool {
    store.rejected.as_deref() != Some(version)
        && store
            .attempts_for(version)
            .is_none_or(|a| now - a.at >= policy::RETRY_AFTER_FAILURE.as_secs() as i64)
}

fn in_login_window(app: &AppHandle) -> bool {
    LOGIN_LAUNCH.load(Ordering::SeqCst)
        && uptime() < policy::LOGIN_WINDOW
        && app.state::<AppState>().overlay.lock().unwrap().capture_id == 0
}

fn gate_input(app: &AppHandle, mode: GateMode, ctx: GateCtx, version: &str) -> GateInput {
    let state = app.state::<AppState>();
    let (capture_active, since_last_use) = {
        let ov = state.overlay.lock().unwrap();
        (ov.active || ov.finishing, ov.last_used.map(|t| t.elapsed()))
    };
    // A wall clock set back reads as "just changed", which only delays the install.
    let set_unchanged_for = state
        .session
        .lock()
        .unwrap()
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| s.changed_at.elapsed().unwrap_or_default());
    GateInput {
        mode,
        capture_active,
        since_last_use,
        smart_paste_busy: crate::actions::smart_paste_busy(),
        nudge_in_flight: crate::actions::nudge_in_flight(),
        last_copy: state.last_copy.lock().unwrap().is_some(),
        set_unchanged_for,
        ui_window_open: app.get_webview_window("settings").is_some() || app.get_webview_window("onboarding").is_some(),
        pins_open: crate::pins::open_count(app) > 0,
        hotkeys_suspended: state.hotkeys_suspended.load(Ordering::SeqCst),
        idle: system::idle(),
        idle_on_previous_tick: ctx.idle_before,
        fullscreen_busy: system::fullscreen_busy(),
        since_resume: ctx.resumed_at.map(|t| t.elapsed()),
        installed_copy: installed_copy(),
        failed_attempts: store::get().attempts_for(version).map_or(0, |a| a.count),
    }
}

/// Asks the update endpoint and remembers the result for the tray and the About tab.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = fetch(app, CHECK_TIMEOUT).await?;
    store::update(|s| s.last_check_at = Some(now_secs()));
    apply_check(app, update.as_ref());
    Ok(update.map(|u| info_of(&u)))
}

/// Always through `updater_builder`, which installs the hook that removes the tray icon and hides the
/// windows before the installer starts.
async fn fetch(app: &AppHandle, timeout: Duration) -> Result<Option<Update>, String> {
    let updater = app.updater_builder().timeout(timeout).build().map_err(|e| e.to_string())?;
    let result = updater.check().await;
    RUNTIME.lock().unwrap().fetched(result.is_ok(), Instant::now());
    result.map_err(|e| e.to_string())
}

fn info_of(update: &Update) -> UpdateInfo {
    UpdateInfo { version: update.version.clone(), notes: update.body.clone() }
}

fn apply_check(app: &AppHandle, update: Option<&Update>) {
    let next = match update {
        Some(u) => {
            let (_, seen) = store::update(|s| s.note_offered(&u.version, now_secs()));
            let auto_after = policy::soak_until(seen, policy::soak_override(&u.raw_json));
            UpdatePhase::Available { info: info_of(u), auto_after }
        }
        None => UpdatePhase::Idle,
    };
    RUNTIME.lock().unwrap().offered(update.map(|u| u.version.as_str()));
    set_phase(app, |current| match current {
        // The re-check inside a running install must not take its phase away.
        UpdatePhase::Installing { .. } => current.clone(),
        _ => next,
    });
}

fn set_phase(app: &AppHandle, next: impl FnOnce(&UpdatePhase) -> UpdatePhase) {
    let changed = {
        let state = app.state::<AppState>();
        let mut slot = state.update.lock().unwrap();
        let next = next(&slot);
        let changed = *slot != next;
        if changed && let UpdatePhase::Available { info, auto_after } = &next {
            log::info!("update {} available, may install by itself from {auto_after}", info.version);
        }
        *slot = next;
        changed
    };
    if changed {
        crate::tray::refresh(app);
        emit_status(app);
    }
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    let phase = app.state::<AppState>().update.lock().unwrap().clone();
    let store = store::get();
    let now = now_secs();
    let (name, info) = match phase {
        UpdatePhase::Idle => ("idle", None),
        UpdatePhase::Available { info, .. } => ("available", Some(info)),
        UpdatePhase::Installing { info } => ("installing", Some(info)),
    };
    let auto_eligible = name == "available"
        && info.as_ref().is_some_and(|i| {
            store.rejected.as_deref() != Some(i.version.as_str())
                && store.attempts_for(&i.version).is_none_or(|a| a.count < policy::MAX_FAILED_ATTEMPTS)
        })
        && installed_copy();
    UpdateStatus { phase: name, info, auto_eligible, recent: recent_update(store.last_update, now) }
}

/// "Hide" drops only the notes; "Updated from" stays for the whole 14 days.
fn recent_update(last: Option<store::LastUpdate>, now: i64) -> Option<RecentUpdate> {
    last.filter(|u| now - u.at < policy::NOTES_SHOWN_FOR.as_secs() as i64).map(|u| RecentUpdate {
        from: u.from,
        version: u.to,
        notes: u.notes.filter(|_| !u.notes_hidden),
        at: u.at,
    })
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit("update:status", status(app));
}

/// About's "Hide" under "What's new".
pub fn hide_notes(app: &AppHandle) {
    store::update(|s| {
        if let Some(last) = &mut s.last_update {
            last.notes_hidden = true;
        }
    });
    emit_status(app);
}

/// "Update and restart" in About and the tray item: no waiting time and no idle rules, only nothing in
/// progress. Does not return on success: the app restarts into the new version.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    install_with(app, GateMode::Manual, None, GateCtx::default(), NORMAL, true).await
}

pub fn install_from_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = install(&app).await {
            log::error!("update failed: {err}");
            let lang = app.state::<AppState>().lang();
            let detail = refusal_detail(lang, &err);
            crate::notice::toast_error(&app, crate::i18n::tr(lang, "toast.updateFailed"), Some(detail));
        }
    });
}

/// A refusal code in the UI language; any other error (network, signature) as the updater reported it.
fn refusal_detail(lang: crate::i18n::Lang, err: &str) -> &str {
    match err {
        REFUSED_BUSY => crate::i18n::tr(lang, "toast.updateBusy"),
        REFUSED_GONE => crate::i18n::tr(lang, "toast.updateGone"),
        _ => err,
    }
}

/// Tray "Quit": a version that waited long enough goes in now (without relaunching) instead of at the next
/// idle moment. Otherwise quits at once.
pub fn quit(app: &AppHandle) {
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(version) = quit_candidate(app) else {
        app.exit(0);
        return;
    };
    // The check and the download can take a few seconds: look gone at once, as a plain Quit does.
    drop(app.remove_tray_by_id(crate::tray::TRAY_ID));
    for win in app.webview_windows().values() {
        let _ = win.hide();
    }
    let app = app.clone();
    // Never on the main thread: on macOS the install can need it for a prompt and waits for it.
    std::thread::spawn(move || {
        let timeouts = Timeouts { check: QUIT_CHECK_TIMEOUT, download: QUIT_DOWNLOAD_TIMEOUT };
        let result =
            tauri::async_runtime::block_on(install_with(&app, GateMode::Quit, Some(&version), GateCtx::default(), timeouts, false));
        if let Err(err) = result {
            log::warn!("update on quit failed: {err}");
        }
        app.exit(0);
    });
}

fn quit_candidate(app: &AppHandle) -> Option<String> {
    if cfg!(debug_assertions) {
        return None;
    }
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.auto_check_updates || !settings.auto_install_updates {
        return None;
    }
    let (version, auto_after) = match &*state.update.lock().unwrap() {
        UpdatePhase::Available { info, auto_after } => (info.version.clone(), *auto_after),
        _ => return None,
    };
    let now = now_secs();
    let waits = RUNTIME.lock().unwrap().install_waits(&version, Instant::now());
    (now >= auto_after && may_retry(&store::get(), &version, now) && !waits).then_some(version)
}

/// Re-checks the endpoint (a release pulled meanwhile is dropped here), downloads if needed, claims the app
/// so no capture or smart paste starts, and installs. Only Manual reports a refusal, as `REFUSED_BUSY` or
/// `REFUSED_GONE`; the background paths log it and try again later. Does not return after a successful
/// install that restarts.
async fn install_with(
    app: &AppHandle,
    mode: GateMode,
    expected: Option<&str>,
    ctx: GateCtx,
    timeouts: Timeouts,
    restart: bool,
) -> Result<(), String> {
    let refuse = |code: &str, reason: String| {
        if mode == GateMode::Manual {
            log::info!("update not installed: {reason}");
            Err(code.to_string())
        } else {
            log::info!("update not installed now ({mode:?}): {reason}");
            Ok(())
        }
    };
    let Some(_installing) = crate::actions::BusyGuard::claim(&INSTALLING) else {
        return refuse(REFUSED_BUSY, "an update is already being installed".into());
    };
    let update = fetch(app, timeouts.check).await?;
    apply_check(app, update.as_ref());
    let Some(update) = update else { return refuse(REFUSED_GONE, "no update available".into()) };
    if expected.is_some_and(|v| v != update.version) {
        return refuse(REFUSED_GONE, format!("now {} is offered, it waits first", update.version));
    }
    let bytes = package(&update, timeouts.download, mode != GateMode::Manual).await?;

    let gate = |claimed: bool| {
        let mut input = gate_input(app, mode, ctx, &update.version);
        if claimed {
            // Our own claims.
            input.capture_active = false;
            input.smart_paste_busy = false;
        }
        policy::install_gate(&input)
    };
    // Manual keeps only the rules for work in progress, so any block there means "busy".
    let blocked = |block: Block| refuse(REFUSED_BUSY, format!("{block:?}"));
    if let Err(block) = gate(false) {
        return blocked(block);
    }
    if !crate::overlay::try_claim_for_update(app) {
        return blocked(Block::Capture);
    }
    let Some(paste_claim) = crate::actions::claim_smart_paste() else {
        crate::overlay::release_update_claim(app);
        return blocked(Block::SmartPaste);
    };
    if let Err(block) = gate(true) {
        drop(paste_claim);
        crate::overlay::release_update_claim(app);
        return blocked(block);
    }

    let set_cleared = app.state::<AppState>().session.lock().unwrap().as_ref().is_some_and(|s| !s.is_empty());
    let (_, count) = store::update(|s| {
        let count = s.attempts_for(&update.version).map_or(0, |a| a.count) + 1;
        s.attempt = Some(store::Attempt {
            to: update.version.clone(),
            count,
            at: now_secs(),
            set_cleared,
            notes: update.body.clone(),
        });
        count
    });
    log::info!("installing update {} ({mode:?}, attempt {count}, set cleared: {set_cleared})", update.version);
    let previous = app.state::<AppState>().update.lock().unwrap().clone();
    set_phase(app, |_| UpdatePhase::Installing { info: info_of(&update) });

    let result = install_now(app, &update, &bytes, restart);
    if result.is_err() {
        // Quit exits right after; nothing to bring back.
        if mode != GateMode::Quit {
            restore_after_failure(app, previous);
        }
        drop(paste_claim);
        crate::overlay::release_update_claim(app);
    }
    result
}

/// Windows: the plugin runs the hook (tray icon removed, windows hidden), starts the silent installer and
/// exits; the installer relaunches the app with the same arguments when `restart` is set. macOS: the bundle
/// is swapped in place and the app restarts from this thread, never the main one, so the single-instance
/// socket is closed before the new instance starts.
fn install_now(app: &AppHandle, update: &Update, bytes: &[u8], restart: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = app;
        update.clone().restart_after_install(restart).install(bytes).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        update.install(bytes).map_err(|e| e.to_string())?;
        if restart {
            app.restart();
        }
        Ok(())
    }
}

/// The hook ran before the installer failed to start: the tray icon is gone and the windows are hidden.
fn restore_after_failure(app: &AppHandle, previous: UpdatePhase) {
    if app.tray_by_id(crate::tray::TRAY_ID).is_none()
        && let Err(err) = crate::tray::build(app)
    {
        log::error!("could not restore the tray icon: {err}");
    }
    #[cfg(windows)]
    {
        for label in ["settings", "onboarding"] {
            if let Some(win) = app.get_webview_window(label) {
                let _ = win.show();
            }
        }
        crate::windows_ui::sync_strip(app);
    }
    set_phase(app, |_| previous);
}

/// The verified package of `update`, downloaded unless it is already in memory. A signature error rejects
/// the version for good; other errors back off (1, 2, 4, then 12 hours) unless the user asked.
async fn package(update: &Update, timeout: Duration, respect_backoff: bool) -> Result<Vec<u8>, String> {
    {
        let rt = RUNTIME.lock().unwrap();
        if let Some((version, bytes)) = &rt.package
            && *version == update.version
        {
            return Ok(bytes.clone());
        }
        if respect_backoff && rt.download_waits(&update.version, Instant::now()) {
            return Err("waiting after a failed download".into());
        }
    }
    let mut request = update.clone();
    request.timeout = Some(timeout);
    match request.download(|_, _| {}, || {}).await {
        Ok(bytes) => {
            RUNTIME.lock().unwrap().downloaded(&update.version, bytes.clone());
            Ok(bytes)
        }
        Err(err) => {
            if is_signature_error(&err) {
                log::error!("update {} has a bad signature, it will not be installed automatically: {err}", update.version);
                store::update(|s| s.rejected = Some(update.version.clone()));
            } else {
                RUNTIME.lock().unwrap().download_failed(&update.version, Instant::now());
            }
            Err(err.to_string())
        }
    }
}

fn is_signature_error(err: &tauri_plugin_updater::Error) -> bool {
    use tauri_plugin_updater::Error;
    matches!(
        err,
        Error::Minisign(_)
            | Error::Base64(_)
            | Error::SignatureUtf8(_)
            | Error::SignedVersionMismatch { .. }
            | Error::MissingSignedVersion
    )
}

/// Shows the "updated" pill once the user is back, then forgets it. Gives up after a week.
fn start_notice_watcher(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(NOTICE_POLL);
            let Some(last) = store::get().last_update.filter(|u| u.notice_pending) else { return };
            if now_secs() - last.at > policy::NOTICE_GIVE_UP.as_secs() as i64 {
                clear_notice_pending();
                return;
            }
            let (capture_active, since_capture) = {
                let state = app.state::<AppState>();
                let ov = state.overlay.lock().unwrap();
                (ov.active || ov.finishing, ov.last_used.map(|t| t.elapsed()))
            };
            let due = policy::notice_due(
                uptime(),
                system::idle(),
                capture_active,
                since_capture,
                crate::actions::nudge_in_flight(),
            );
            if !due {
                continue;
            }
            let has_notes = last.notes.as_deref().is_some_and(policy::notes_have_text);
            if crate::notice::show_updated(&app, &last.to, has_notes, last.set_cleared) {
                clear_notice_pending();
                return;
            }
        }
    });
}

fn clear_notice_pending() {
    store::update(|s| {
        if let Some(last) = &mut s.last_update {
            last.notice_pending = false;
        }
    });
}

fn installed_copy() -> bool {
    *INSTALLED_COPY.get_or_init(system::installed_copy)
}

fn uptime() -> Duration {
    STARTED.get().map(Instant::elapsed).unwrap_or_default()
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_waits_after_a_failure_and_never_for_a_rejected_version() {
        let hours = |h: i64| h * 3600;
        let mut s = store::UpdateStore::default();
        assert!(may_retry(&s, "0.2.0", 0));
        s.attempt = Some(store::Attempt { to: "0.2.0".into(), count: 1, at: 0, ..Default::default() });
        assert!(!may_retry(&s, "0.2.0", hours(6) - 1));
        assert!(may_retry(&s, "0.2.0", hours(6)));
        assert!(may_retry(&s, "0.3.0", 0), "another version");
        s.rejected = Some("0.2.0".into());
        assert!(!may_retry(&s, "0.2.0", hours(100)));
    }

    #[test]
    fn background_installs_wait_after_a_failed_check_or_download() {
        let t0 = Instant::now();
        let hour = Duration::from_secs(3600);
        let mut rt = Runtime::new();
        assert!(!rt.install_waits("0.2.0", t0));

        rt.fetched(false, t0);
        assert!(rt.install_waits("0.2.0", t0 + hour - Duration::from_secs(1)), "offline: no request every minute");
        assert!(!rt.install_waits("0.2.0", t0 + hour));
        rt.fetched(true, t0);
        assert!(!rt.check_waits(t0));

        rt.download_failed("0.2.0", t0);
        assert!(rt.install_waits("0.2.0", t0 + Duration::from_secs(60)));
        assert!(!rt.install_waits("0.2.0", t0 + hour));
        assert!(!rt.install_waits("0.3.0", t0), "the backoff belongs to its version");
        rt.download_failed("0.2.0", t0);
        assert!(rt.download_waits("0.2.0", t0 + hour), "second failure: 2 h");

        rt.downloaded("0.2.0", vec![1]);
        rt.download_failed("0.3.0", t0);
        assert!(!rt.install_waits("0.2.0", t0), "a package in memory needs no download");
        rt.fetched(false, t0);
        assert!(rt.install_waits("0.2.0", t0), "but every install starts with a check");
    }

    #[test]
    fn a_new_offer_drops_the_old_package_and_backoff() {
        let t0 = Instant::now();
        let mut rt = Runtime::new();
        for _ in 0..4 {
            rt.download_failed("0.2.0", t0);
        }
        rt.offered(Some("0.2.0"));
        assert_eq!(rt.download_failures, 4, "the same version keeps its backoff");
        rt.offered(Some("0.3.0"));
        assert_eq!((rt.download_failures, rt.next_download_at, rt.backoff_version.as_deref()), (0, None, None));
        rt.download_failed("0.3.0", t0);
        assert_eq!(rt.download_failures, 1, "starts at 1 h again");
        rt.offered(None);
        assert_eq!(rt.download_failures, 0, "pulled release");

        rt.downloaded("0.3.0", vec![1]);
        rt.offered(Some("0.3.0"));
        assert!(rt.package.is_some());
        rt.offered(Some("0.4.0"));
        assert!(rt.package.is_none());
    }

    #[test]
    fn hiding_the_notes_keeps_updated_from() {
        let last = store::LastUpdate {
            from: Some("0.1.0".into()),
            to: "0.2.0".into(),
            notes: Some("n".into()),
            at: 100,
            ..Default::default()
        };
        let recent = recent_update(Some(last.clone()), 200).unwrap();
        assert_eq!((recent.from.as_deref(), recent.notes.as_deref()), (Some("0.1.0"), Some("n")));
        let hidden = store::LastUpdate { notes_hidden: true, ..last.clone() };
        let recent = recent_update(Some(hidden), 200).unwrap();
        assert_eq!((recent.from.as_deref(), recent.notes), (Some("0.1.0"), None));
        let days = policy::NOTES_SHOWN_FOR.as_secs() as i64;
        assert!(recent_update(Some(last.clone()), 100 + days - 1).is_some());
        assert_eq!(recent_update(Some(last), 100 + days), None, "gone after 14 days");
        assert_eq!(recent_update(None, 0), None);
    }

    #[test]
    fn refusals_are_shown_in_the_ui_language() {
        use crate::i18n::Lang;
        for lang in [Lang::Ru, Lang::En] {
            for code in [REFUSED_BUSY, REFUSED_GONE] {
                let text = refusal_detail(lang, code);
                assert!(!text.is_empty() && text != code, "{code} in {lang:?}");
            }
        }
        assert_ne!(refusal_detail(Lang::Ru, REFUSED_BUSY), refusal_detail(Lang::En, REFUSED_BUSY));
        assert_eq!(refusal_detail(Lang::Ru, "error sending request"), "error sending request");
    }

    #[test]
    fn status_serializes_for_the_frontend() {
        let status = UpdateStatus {
            phase: "available",
            info: Some(UpdateInfo { version: "0.2.0".into(), notes: None }),
            auto_eligible: true,
            recent: Some(RecentUpdate { from: None, version: "0.1.1".into(), notes: Some("n".into()), at: 5 }),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["phase"], "available");
        assert_eq!(json["autoEligible"], true);
        assert_eq!(json["info"]["version"], "0.2.0");
        assert_eq!(json["recent"]["from"], serde_json::Value::Null);
        assert_eq!(json["recent"]["at"], 5);
    }
}
