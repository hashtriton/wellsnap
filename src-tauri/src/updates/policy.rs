//! Pure decisions of the updater: when to check, when a release has waited long enough, whether it is safe to
//! install right now, and what a start after an install means. No I/O here, so every rule is table-tested.
//! Timestamps are Unix seconds (wall clock), spans are `Duration`.

use std::cmp::Ordering;
use std::time::Duration;

pub const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);
/// A scheduled check that failed (offline at that moment) is tried again this much later.
pub const CHECK_RETRY: Duration = Duration::from_secs(60 * 60);
/// Two updater ticks (one a minute) further apart than this on the wall clock mean the computer slept.
pub const WAKE_GAP: Duration = Duration::from_secs(3 * 60);
/// No install right after a wake: the user is probably about to sit down.
pub const RESUME_GRACE: Duration = Duration::from_secs(2 * 60);
/// A new version waits this long plus the install's own 0-23 h bucket, so a bad release can still be pulled.
pub const SOAK_HOURS: u64 = 24;
pub const BUCKETS: u8 = 24;
pub const UNUSED_FOR: Duration = Duration::from_secs(10 * 60);
pub const SET_UNCHANGED_FOR: Duration = Duration::from_secs(24 * 60 * 60);
pub const IDLE_FOR: Duration = Duration::from_secs(5 * 60);
pub const MAX_FAILED_ATTEMPTS: u32 = 3;
pub const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(6 * 60 * 60);
/// After a login launch, an update may go in during this first stretch, before the first capture.
pub const LOGIN_WINDOW: Duration = Duration::from_secs(10 * 60);
/// About shows the notes of the installed version for this long.
pub const NOTES_SHOWN_FOR: Duration = Duration::from_secs(14 * 24 * 60 * 60);
/// The "updated" notice is dropped if it could not be shown for this long.
pub const NOTICE_GIVE_UP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const NOTICE_MIN_UPTIME: Duration = Duration::from_secs(15);
const NOTICE_USER_BACK: Duration = Duration::from_secs(3);
const NOTICE_AFTER_CAPTURE: Duration = Duration::from_secs(4);

fn secs(d: Duration) -> i64 {
    i64::try_from(d.as_secs()).unwrap_or(i64::MAX)
}

/// A clock that went backwards (manual change, a bad RTC) counts as due rather than blocking checks for days.
pub fn check_due(now: i64, last: Option<i64>, interval: Duration) -> bool {
    match last {
        None => true,
        Some(last) => now < last || now - last >= secs(interval),
    }
}

/// Wall clock jumped forward between two ticks: the computer was asleep. A clock set back is not a wake.
pub fn woke_up(previous_tick: i64, now: i64) -> bool {
    now - previous_tick > secs(WAKE_GAP)
}

pub fn just_resumed(since_resume: Option<Duration>) -> bool {
    since_resume.is_some_and(|d| d < RESUME_GRACE)
}

/// When version V, first seen at `first_seen`, may install automatically. `override_hours` comes from the
/// release's latest.json (`"wellsnap": {"soakHours": N}`) and replaces both the base and the bucket.
pub fn soak_until(first_seen: i64, bucket: u8, override_hours: Option<u64>) -> i64 {
    let hours = override_hours.unwrap_or(SOAK_HOURS + u64::from(bucket.min(BUCKETS - 1)));
    first_seen.saturating_add(i64::try_from(hours.saturating_mul(3600)).unwrap_or(i64::MAX))
}

/// Reads the optional `"wellsnap": {"soakHours": N}` of latest.json; anything else means "not set".
pub fn soak_override(raw_json: &serde_json::Value) -> Option<u64> {
    raw_json.get("wellsnap")?.get("soakHours")?.as_u64()
}

/// Wait after the n-th download failure in a row.
pub fn backoff(failures: u32) -> Duration {
    let hours = match failures {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        _ => 12,
    };
    Duration::from_secs(hours * 60 * 60)
}

/// `GetTickCount` and `LASTINPUTINFO.dwTime` are 32-bit millisecond counters that wrap after 49.7 days.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub fn idle_from_ticks(now: u32, last_input: u32) -> Duration {
    Duration::from_millis(u64::from(now.wrapping_sub(last_input)))
}

/// "X.Y.Z" with an optional "-pre" / "+build" suffix. A pre-release sorts before its release; two
/// pre-releases of the same version compare as text, which is enough for the versions this app ships.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    fn parse(v: &str) -> Option<([u64; 3], Option<&str>)> {
        let v = v.trim().trim_start_matches('v');
        let v = v.split('+').next()?;
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (v, None),
        };
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let triple = [parts.next()??, parts.next()??, parts.next()??];
        parts.next().is_none().then_some((triple, pre))
    }
    let (a, a_pre) = parse(a)?;
    let (b, b_pre) = parse(b)?;
    Some(a.cmp(&b).then_with(|| match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(y),
    }))
}

/// Which rules apply: the idle install, the one shortly after a login launch, the one on Quit, or the user's
/// own "Update and restart".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GateMode {
    #[default]
    Auto,
    Login,
    Quit,
    Manual,
}

#[derive(Debug, Clone, Default)]
pub struct GateInput {
    pub mode: GateMode,
    /// A capture is on screen or its finish is running.
    pub capture_active: bool,
    /// Since a capture last started or ended; None when WellSnap was not used since start.
    pub since_last_use: Option<Duration>,
    pub smart_paste_busy: bool,
    /// A donation reminder is scheduled or on screen.
    pub nudge_in_flight: bool,
    /// A single copy with separate text is waiting for smart paste.
    pub last_copy: bool,
    /// None when there is no snapshot set.
    pub set_unchanged_for: Option<Duration>,
    /// Settings or onboarding is open.
    pub ui_window_open: bool,
    /// Settings records a hotkey.
    pub hotkeys_suspended: bool,
    /// Time since the last keyboard or mouse input; None when the system does not tell.
    pub idle: Option<Duration>,
    /// The previous tick also saw `IDLE_FOR`, so one quiet minute is not enough.
    pub idle_on_previous_tick: bool,
    /// Windows: a full-screen app, a presentation, or "busy" (Focus assist sees the same).
    pub fullscreen_busy: bool,
    pub since_resume: Option<Duration>,
    /// An installed copy that may replace itself (not a dev build, not a copy run from a disk image).
    pub installed_copy: bool,
    /// Failed automatic attempts for this version.
    pub failed_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Capture,
    RecentlyUsed,
    SmartPaste,
    Nudge,
    LastCopy,
    SnapshotSet,
    Window,
    HotkeyRecording,
    NotIdle,
    Fullscreen,
    JustResumed,
    NotInstalled,
    TooManyFailures,
}

/// All conditions under which an update may replace the running app now. Auto checks everything; Login drops
/// the recent-use, snapshot-set and idle rules (the set lives in memory and is empty after a login); Quit
/// and Manual keep only what would lose the user's work in progress.
pub fn install_gate(input: &GateInput) -> Result<(), Block> {
    use GateMode::*;
    let mode = input.mode;
    let auto = mode == Auto;
    let unattended = matches!(mode, Auto | Login);
    let check = |applies: bool, blocked: bool, block: Block| if applies && blocked { Err(block) } else { Ok(()) };

    check(true, input.capture_active, Block::Capture)?;
    check(true, input.smart_paste_busy, Block::SmartPaste)?;
    check(auto, input.since_last_use.is_some_and(|d| d < UNUSED_FOR), Block::RecentlyUsed)?;
    check(unattended, input.nudge_in_flight, Block::Nudge)?;
    check(unattended, input.last_copy, Block::LastCopy)?;
    check(auto, input.set_unchanged_for.is_some_and(|d| d < SET_UNCHANGED_FOR), Block::SnapshotSet)?;
    check(unattended, input.ui_window_open, Block::Window)?;
    check(unattended, input.hotkeys_suspended, Block::HotkeyRecording)?;
    let idle = input.idle.is_some_and(|d| d >= IDLE_FOR) && input.idle_on_previous_tick;
    check(auto, !idle, Block::NotIdle)?;
    check(unattended, input.fullscreen_busy, Block::Fullscreen)?;
    check(unattended, just_resumed(input.since_resume), Block::JustResumed)?;
    check(mode != Manual, !input.installed_copy, Block::NotInstalled)?;
    check(mode != Manual, input.failed_attempts >= MAX_FAILED_ATTEMPTS, Block::TooManyFailures)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOutcome {
    /// A fresh install: no notice.
    FirstRun,
    Same,
    /// `from` is None for a copy updated from a version that kept no marker.
    Updated { from: Option<String> },
    /// An install of `to` was started but this is still the old version.
    InstallFailed { to: String },
    /// Downgraded by hand: no notice.
    Older,
}

/// What this start means, from the version marker of the previous run and the attempt marker written right
/// before an install. Without a marker, a finished onboarding means the previous run was an older version.
/// A version change wins over an attempt at a still newer one (installed by hand after that attempt failed).
pub fn startup_outcome(
    last_run: Option<&str>,
    current: &str,
    attempt_to: Option<&str>,
    onboarding_done: bool,
) -> StartupOutcome {
    let changed = last_run.is_some_and(|last| compare_versions(last, current) == Some(Ordering::Less));
    if !changed
        && let Some(to) = attempt_to
        && compare_versions(current, to) == Some(Ordering::Less)
    {
        return StartupOutcome::InstallFailed { to: to.to_string() };
    }
    match last_run {
        None if onboarding_done => StartupOutcome::Updated { from: None },
        None => StartupOutcome::FirstRun,
        Some(last) => match compare_versions(last, current) {
            Some(Ordering::Less) => StartupOutcome::Updated { from: Some(last.to_string()) },
            Some(Ordering::Greater) => StartupOutcome::Older,
            // Equal, or a marker that does not parse.
            _ => StartupOutcome::Same,
        },
    }
}

/// Whether About will show anything for these release notes: the same reading as `pickNotes` in
/// src/settings/notes.ts. HTML comments are dropped; with a "Русский" or "English" section only the text in
/// those sections counts, without them the whole text does.
pub fn notes_have_text(notes: &str) -> bool {
    let text = strip_html_comments(notes);
    let mut has_sections = false;
    let mut in_section = false;
    let mut section_text = false;
    for line in text.lines() {
        if let Some(title) = heading_title(line) {
            in_section = matches!(title.to_lowercase().as_str(), "русский" | "english");
            has_sections |= in_section;
            continue;
        }
        section_text |= in_section && !line.trim().is_empty();
    }
    if has_sections { section_text } else { !text.trim().is_empty() }
}

/// "# Title", "## Title ##" or "### Title"; deeper headings are text.
fn heading_title(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    let hashes = line.len() - rest.len();
    ((1..=3).contains(&hashes) && rest.starts_with(char::is_whitespace))
        .then(|| rest.trim().trim_end_matches('#').trim_end())
}

/// An unterminated "<!--" is left as text.
fn strip_html_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        let Some(len) = rest[start + 4..].find("-->") else { break };
        out.push_str(&rest[..start]);
        rest = &rest[start + 4 + len + 3..];
    }
    out.push_str(rest);
    out
}

/// The "updated" notice waits until the user is back at the computer and nothing else is on screen.
pub fn notice_due(
    uptime: Duration,
    idle: Option<Duration>,
    capture_active: bool,
    since_capture: Option<Duration>,
    nudge_in_flight: bool,
) -> bool {
    uptime >= NOTICE_MIN_UPTIME
        && idle.is_none_or(|d| d < NOTICE_USER_BACK)
        && !capture_active
        && since_capture.is_none_or(|d| d >= NOTICE_AFTER_CAPTURE)
        && !nudge_in_flight
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600;

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    #[test]
    fn checks_are_due_after_the_interval_or_when_the_clock_went_back() {
        let cases = [
            (1_000, None, true),
            (1_000 + 12 * H - 1, Some(1_000), false),
            (1_000 + 12 * H, Some(1_000), true),
            (1_000, Some(1_000), false),
            (999, Some(1_000), true),
            (1_000 - 30 * 24 * H, Some(1_000), true),
        ];
        for (now, last, due) in cases {
            assert_eq!(check_due(now, last, CHECK_INTERVAL), due, "now {now}, last {last:?}");
        }
    }

    #[test]
    fn wake_detection_and_grace() {
        assert!(!woke_up(1_000, 1_060));
        assert!(!woke_up(1_000, 1_180));
        assert!(woke_up(1_000, 1_181));
        assert!(!woke_up(1_000, 500), "a clock set back is not a wake");
        assert!(!just_resumed(None));
        assert!(just_resumed(Some(Duration::ZERO)));
        assert!(just_resumed(Some(mins(2) - Duration::from_secs(1))));
        assert!(!just_resumed(Some(mins(2))));
    }

    #[test]
    fn soak_is_base_plus_bucket_or_the_override() {
        let seen = 10_000;
        let cases: [(u8, Option<u64>, i64); 6] = [
            (0, None, seen + 24 * H),
            (23, None, seen + 47 * H),
            (200, None, seen + 47 * H),
            (23, Some(0), seen),
            (0, Some(2), seen + 2 * H),
            (5, Some(u64::MAX), i64::MAX),
        ];
        for (bucket, over, until) in cases {
            assert_eq!(soak_until(seen, bucket, over), until, "bucket {bucket}, override {over:?}");
        }
        assert_eq!(soak_until(seen, 23, Some(0)), seen, "override 0 installs at once");
    }

    #[test]
    fn soak_override_is_read_only_from_its_own_field() {
        let read = |json: &str| soak_override(&serde_json::from_str(json).unwrap());
        assert_eq!(read(r#"{"version":"0.2.0","wellsnap":{"soakHours":0}}"#), Some(0));
        assert_eq!(read(r#"{"wellsnap":{"soakHours":6}}"#), Some(6));
        assert_eq!(read(r#"{"version":"0.2.0"}"#), None, "missing field");
        assert_eq!(read(r#"{"wellsnap":{}}"#), None);
        assert_eq!(read(r#"{"wellsnap":{"soakHours":"0"}}"#), None);
        assert_eq!(read(r#"{"wellsnap":{"soakHours":-1}}"#), None);
        assert_eq!(read(r#"{"soakHours":0}"#), None);
    }

    #[test]
    fn download_backoff_grows_and_caps() {
        let hours = [(0, 0), (1, 1), (2, 2), (3, 4), (4, 12), (40, 12)];
        for (failures, h) in hours {
            assert_eq!(backoff(failures), Duration::from_secs(h * 3600), "{failures} failures");
        }
    }

    #[test]
    fn idle_ticks_wrap_around() {
        assert_eq!(idle_from_ticks(10_000, 4_000), Duration::from_millis(6_000));
        assert_eq!(idle_from_ticks(5, u32::MAX - 4), Duration::from_millis(10), "counter wrapped after 49.7 days");
        assert_eq!(idle_from_ticks(7, 7), Duration::ZERO);
    }

    #[test]
    fn versions_compare_numerically() {
        use Ordering::*;
        let cases = [
            ("0.1.0", "0.2.0", Some(Less)),
            ("0.10.0", "0.9.9", Some(Greater)),
            ("1.2.3", "1.2.3", Some(Equal)),
            ("v1.2.3", "1.2.3", Some(Equal)),
            ("1.2.3+build.5", "1.2.3", Some(Equal)),
            ("1.2.3-beta.1", "1.2.3", Some(Less)),
            ("1.2.3-beta.1", "1.2.3-beta.2", Some(Less)),
            ("1.2", "1.2.0", None),
            ("1.2.3.4", "1.2.3", None),
            ("x", "1.0.0", None),
        ];
        for (a, b, ord) in cases {
            assert_eq!(compare_versions(a, b), ord, "{a} vs {b}");
        }
    }

    fn clear(mode: GateMode) -> GateInput {
        GateInput {
            mode,
            since_last_use: Some(mins(60)),
            idle: Some(mins(30)),
            idle_on_previous_tick: true,
            since_resume: Some(mins(60)),
            installed_copy: true,
            ..Default::default()
        }
    }

    #[test]
    fn gate_all_clear_in_every_mode() {
        for mode in [GateMode::Auto, GateMode::Login, GateMode::Quit, GateMode::Manual] {
            assert_eq!(install_gate(&clear(mode)), Ok(()), "{mode:?}");
        }
        let never_used = GateInput { since_last_use: None, since_resume: None, ..clear(GateMode::Auto) };
        assert_eq!(install_gate(&never_used), Ok(()), "never used, never slept");
        let old_set = GateInput { set_unchanged_for: Some(SET_UNCHANGED_FOR), ..clear(GateMode::Auto) };
        assert_eq!(install_gate(&old_set), Ok(()), "a set unchanged for 24 h does not block");
    }

    #[test]
    fn gate_blocks_each_condition_alone() {
        type Change = fn(&mut GateInput);
        // (change, block, applies in [Auto, Login, Quit, Manual])
        let cases: [(Change, Block, [bool; 4]); 15] = [
            (|i| i.capture_active = true, Block::Capture, [true, true, true, true]),
            (|i| i.smart_paste_busy = true, Block::SmartPaste, [true, true, true, true]),
            (|i| i.since_last_use = Some(mins(9)), Block::RecentlyUsed, [true, false, false, false]),
            (|i| i.nudge_in_flight = true, Block::Nudge, [true, true, false, false]),
            (|i| i.last_copy = true, Block::LastCopy, [true, true, false, false]),
            (|i| i.set_unchanged_for = Some(mins(60)), Block::SnapshotSet, [true, false, false, false]),
            (|i| i.ui_window_open = true, Block::Window, [true, true, false, false]),
            (|i| i.hotkeys_suspended = true, Block::HotkeyRecording, [true, true, false, false]),
            (|i| i.idle = Some(mins(4)), Block::NotIdle, [true, false, false, false]),
            (|i| i.idle = None, Block::NotIdle, [true, false, false, false]),
            (|i| i.idle_on_previous_tick = false, Block::NotIdle, [true, false, false, false]),
            (|i| i.fullscreen_busy = true, Block::Fullscreen, [true, true, false, false]),
            (|i| i.since_resume = Some(mins(1)), Block::JustResumed, [true, true, false, false]),
            (|i| i.installed_copy = false, Block::NotInstalled, [true, true, true, false]),
            (|i| i.failed_attempts = MAX_FAILED_ATTEMPTS, Block::TooManyFailures, [true, true, true, false]),
        ];
        let modes = [GateMode::Auto, GateMode::Login, GateMode::Quit, GateMode::Manual];
        for (change, block, applies) in cases {
            for (mode, applies) in modes.into_iter().zip(applies) {
                let mut input = clear(mode);
                change(&mut input);
                let expected = if applies { Err(block) } else { Ok(()) };
                assert_eq!(install_gate(&input), expected, "{block:?} in {mode:?}");
            }
        }
        let below = GateInput { failed_attempts: MAX_FAILED_ATTEMPTS - 1, ..clear(GateMode::Auto) };
        assert_eq!(install_gate(&below), Ok(()));
    }

    #[test]
    fn startup_outcomes() {
        use StartupOutcome::*;
        let cases = [
            (None, "0.2.0", None, false, FirstRun),
            (None, "0.2.0", None, true, Updated { from: None }),
            (Some("0.2.0"), "0.2.0", None, true, Same),
            (Some("0.1.0"), "0.2.0", None, true, Updated { from: Some("0.1.0".into()) }),
            (Some("0.1.0"), "0.2.0", Some("0.2.0"), true, Updated { from: Some("0.1.0".into()) }),
            (Some("0.1.0"), "0.1.0", Some("0.2.0"), true, InstallFailed { to: "0.2.0".into() }),
            (None, "0.1.0", Some("0.2.0"), true, InstallFailed { to: "0.2.0".into() }),
            (Some("0.3.0"), "0.2.0", None, true, Older),
            (Some("garbage"), "0.2.0", None, true, Same),
            (Some("0.1.0"), "0.3.0", Some("0.2.0"), true, Updated { from: Some("0.1.0".into()) }),
            // The attempt at 0.3.0 failed, then 0.2.0 was installed by hand.
            (Some("0.1.0"), "0.2.0", Some("0.3.0"), true, Updated { from: Some("0.1.0".into()) }),
            (Some("0.2.0"), "0.2.0", Some("0.3.0"), true, InstallFailed { to: "0.3.0".into() }),
        ];
        for (last, current, attempt, onboarded, outcome) in cases {
            assert_eq!(startup_outcome(last, current, attempt, onboarded), outcome, "{last:?} -> {current}, {attempt:?}");
        }
    }

    #[test]
    fn notes_count_only_when_about_shows_them() {
        let cases = [
            ("### Русский\n- Исправлено\n\n### English\n- Fixed", true),
            ("### English\nFixes", true),
            ("## ENGLISH ##\r\nA", true),
            ("### Русский\n\n### English\n", false),
            ("<!--\nCopy to vX.Y.Z.md\n-->\n\n### Русский\n\n### English\n", false),
            ("<!-- note -->\n### English\n- Fixed", true),
            ("### English\n<!-- later -->\n", false),
            ("Just fixes.", true),
            ("## Other\ntext", true),
            ("<!-- only a comment -->", false),
            ("<!-- unterminated", true),
            ("", false),
            ("  \n ", false),
            ("### Русский\n#### Подробнее\n", true),
            ("### Русский\n## Downloads\nWindows", false),
        ];
        for (notes, shown) in cases {
            assert_eq!(notes_have_text(notes), shown, "{notes:?}");
        }
    }

    #[test]
    fn notice_waits_for_the_user() {
        let s = Duration::from_secs;
        let cases = [
            (s(20), Some(s(1)), false, None, false, true),
            (s(14), Some(s(1)), false, None, false, false),
            (s(20), Some(s(3)), false, None, false, false),
            (s(20), None, false, None, false, true),
            (s(20), Some(s(0)), true, None, false, false),
            (s(20), Some(s(0)), false, Some(s(3)), false, false),
            (s(20), Some(s(0)), false, Some(s(4)), false, true),
            (s(20), Some(s(0)), false, None, true, false),
        ];
        for (uptime, idle, capture, since, nudge, due) in cases {
            assert_eq!(notice_due(uptime, idle, capture, since, nudge), due, "{uptime:?} {idle:?} {capture} {since:?} {nudge}");
        }
    }
}
