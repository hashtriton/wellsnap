//! Which program paste with text is pasting into, and how that program takes text and pictures. Pure: the
//! platform code reads the executable (bundle id on macOS) and, for browsers only, the window title.
//!
//! The title is used once per paste and never logged or stored: it often holds a chat or mail subject. Log the
//! target id instead. The public matrix in docs/paste-targets*.md must list every row of `REGISTRY` with the
//! same status; the `matrix_matches_registry` test checks that.
#![allow(dead_code)] // Until WP4b moves smart paste in actions.rs onto `detect` and `effective`.

use super::plan::{Class, Profile};
use crate::i18n::Lang;
use App::{Has, Is};
use Class::{Manual, OnePicture, RichHtml, Sequential, TextOnly};
use Match::{Contains, Ends, Exact, Segment};
use Status::{Active, Planned};

/// The foreground program, as the platform code sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForegroundInfo {
    /// Executable name (Windows, e.g. "telegram.exe") or bundle id (macOS).
    pub app: String,
    /// Full executable path, lowercase (Windows only; empty on macOS). Tells a generic "browser.exe" apart.
    pub path: String,
    /// Window title, read for browsers only.
    pub title: Option<String>,
}

/// The page engine of a browser; `Native` for every other program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Chromium,
    Gecko,
    WebKit,
    Native,
}

/// Whether a row's profile is used. Release builds use `Verified` and `Active` rows only; a `Planned` row
/// pastes like an unknown program until someone checks it by hand. Debug builds use `Planned` rows too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Checked by hand: the date (YYYY-MM-DD) and the program, system and WellSnap versions, as the matrix
    /// shows them.
    Verified { date: &'static str, version: &'static str },
    /// Shipped behaviour that is not checked for this release yet.
    Active,
    Planned,
}

/// An executable name or bundle id, lowercase.
#[derive(Debug, Clone, Copy)]
pub enum App {
    Is(&'static str),
    Has(&'static str),
}

/// A browser title rule, lowercase. `Segment` is a part of the title between separators such as " - ".
#[derive(Debug, Clone, Copy)]
pub enum Match {
    Exact(&'static str),
    Segment(&'static str),
    Ends(&'static str),
    Contains(&'static str),
}

#[derive(Debug)]
pub struct Target {
    /// Stable id: the log, the matrix and the tests use it.
    pub id: &'static str,
    /// Display name (RU, EN) for notices.
    pub name: (&'static str, &'static str),
    pub apps: &'static [App],
    /// Checked only when a browser is in front.
    pub titles: &'static [Match],
    pub profile: Profile,
    pub status: Status,
}

impl Target {
    pub fn name(&self, lang: Lang) -> &'static str {
        if lang == Lang::Ru { self.name.0 } else { self.name.1 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Detected {
    pub target: &'static Target,
    pub engine: Engine,
}

pub const UNKNOWN: &str = "unknown";
/// An unknown page in a Gecko browser: Firefox reads only the first file of a pasted file list.
pub const GECKO_ENGINE: &str = "gecko_engine";

/// Telegram limits: 10 media per album, 1024 caption characters (4096 with Premium, unknown to us).
const TELEGRAM_ALBUM_MAX: usize = 10;
const TELEGRAM_CAPTION_MAX: usize = 1024;

const TELEGRAM: Profile = Profile {
    caption_max: Some(TELEGRAM_CAPTION_MAX),
    album_max: Some(TELEGRAM_ALBUM_MAX),
    ..Profile::of(Class::Album)
};
/// Telegram Web splits a large set into albums itself and sends the text before them.
const TELEGRAM_WEB: Profile = Profile { caption_max: Some(TELEGRAM_CAPTION_MAX), ..Profile::of(Class::Album) };
const ATTACH: Profile = Profile::of(Class::Attach);

const fn attach(max_items: usize) -> Profile {
    Profile { max_items: Some(max_items), ..Profile::of(Class::Attach) }
}

const fn ai_chat(max_items: usize) -> Profile {
    Profile { legend: true, ..attach(max_items) }
}

const fn row(
    id: &'static str,
    name: &'static str,
    apps: &'static [App],
    titles: &'static [Match],
    profile: Profile,
    status: Status,
) -> Target {
    Target { id, name: (name, name), apps, titles, profile, status }
}

/// Every known target, in matching order: the first row whose app or title rule matches wins. The rows follow
/// the sections of the public matrix.
pub static REGISTRY: &[Target] = &[
    // Messengers.
    row("tg_desktop", "Telegram", &[Has("telegram")], &[], TELEGRAM, Active),
    row(
        "tg_fork",
        "Telegram",
        &[Is("ayugram.exe"), Is("kotatogram.exe"), Is("64gram.exe")],
        &[],
        TELEGRAM,
        Planned,
    ),
    row("tg_web_k", "Telegram Web", &[], &[Exact("telegram web")], TELEGRAM_WEB, Planned),
    row("tg_web_a", "Telegram Web", &[], &[Exact("telegram")], TELEGRAM_WEB, Planned),
    row(
        "whatsapp",
        "WhatsApp",
        &[Is("whatsapp.root.exe"), Is("whatsapp.exe")],
        &[Exact("whatsapp"), Exact("whatsapp web")],
        ATTACH,
        Planned,
    ),
    row("whatsapp_mac", "WhatsApp", &[Is("net.whatsapp.whatsapp")], &[], ATTACH, Planned),
    row("max", "MAX", &[], &[Exact("max"), Exact("веб-версия max")], ATTACH, Planned),
    Target {
        id: "vk_messages",
        name: ("ВКонтакте", "VK"),
        apps: &[],
        titles: &[Exact("мессенджер"), Exact("messenger")],
        profile: attach(10),
        status: Planned,
    },
    Target {
        id: "yandex_messenger",
        name: ("Яндекс Мессенджер", "Yandex Messenger"),
        apps: &[],
        titles: &[Segment("яндекс мессенджер"), Segment("yandex messenger")],
        profile: ATTACH,
        status: Planned,
    },
    row("discord", "Discord", &[Is("discord.exe"), Is("com.hnc.discord")], &[Segment("discord")], attach(10), Planned),
    row(
        "slack",
        "Slack",
        &[Is("slack.exe"), Is("com.tinyspeck.slackmacgap")],
        &[Segment("slack")],
        attach(10),
        Planned,
    ),
    row(
        "teams",
        "Microsoft Teams",
        &[Is("ms-teams.exe"), Is("com.microsoft.teams2")],
        &[Segment("microsoft teams")],
        Profile::of(Sequential),
        Planned,
    ),
    row("viber", "Viber", &[Is("viber.exe"), Is("com.viber.osx")], &[], ATTACH, Planned),
    row("zoom", "Zoom", &[Is("zoom.exe"), Is("us.zoom.xos")], &[], ATTACH, Planned),
    // Mail.
    row("outlook_classic", "Outlook", &[Is("outlook.exe")], &[], Profile::of(RichHtml), Planned),
    row("outlook_new", "Outlook", &[Is("olk.exe")], &[], ATTACH, Planned),
    row("outlook_mac", "Outlook", &[Is("com.microsoft.outlook")], &[], Profile::DEFAULT, Planned),
    row("outlook_web", "Outlook", &[], &[Segment("outlook")], ATTACH, Planned),
    row(
        "thunderbird",
        "Thunderbird",
        &[Is("thunderbird.exe"), Is("betterbird.exe"), Is("org.mozilla.thunderbird")],
        &[],
        Profile::of(RichHtml),
        Planned,
    ),
    row(
        "emclient",
        "eM Client",
        &[Is("mailclient.exe"), Is("com.emclient.mail.client")],
        &[],
        Profile::of(Sequential),
        Planned,
    ),
    row("thebat", "The Bat!", &[Is("thebat64.exe"), Is("thebat.exe")], &[], Profile::of(Sequential), Planned),
    row("apple_mail", "Apple Mail", &[Is("com.apple.mail")], &[], ATTACH, Planned),
    row("gmail", "Gmail", &[], &[Segment("gmail")], Profile::of(Sequential), Planned),
    Target {
        id: "yandex_mail",
        name: ("Яндекс Почта", "Yandex Mail"),
        apps: &[],
        titles: &[
            Contains("яндекс почта"),
            Contains("яндекс.почта"),
            Contains("яндекс 360 почта"),
            Contains("yandex mail"),
            Contains("yandex.mail"),
        ],
        profile: Profile::of(Sequential),
        status: Planned,
    },
    row("mailru", "Mail.ru", &[], &[Contains("почта mail")], Profile::of(Sequential), Planned),
    // Social networks.
    Target {
        id: "vk_social",
        name: ("ВКонтакте", "VK"),
        apps: &[],
        titles: &[Segment("вконтакте"), Segment("vk")],
        profile: attach(10),
        status: Planned,
    },
    Target {
        id: "ok",
        name: ("Одноклассники", "OK"),
        apps: &[],
        titles: &[Segment("одноклассники"), Segment("ok")],
        profile: ATTACH,
        status: Planned,
    },
    row("facebook", "Facebook", &[], &[Segment("facebook")], ATTACH, Planned),
    row("x", "X", &[], &[Ends(" / x")], attach(4), Planned),
    row("linkedin", "LinkedIn", &[], &[Segment("linkedin")], attach(20), Planned),
    // The Instagram web composer is not known to take pasted pictures; no profile until someone checks it.
    row("instagram", "Instagram", &[], &[Segment("instagram")], Profile::DEFAULT, Planned),
    row("threads", "Threads", &[], &[Segment("threads")], attach(20), Planned),
    row("reddit", "Reddit", &[], &[Segment("reddit")], attach(20), Planned),
    row("pikabu", "Пикабу", &[], &[Segment("пикабу")], Profile::of(OnePicture), Planned),
    row("habr", "Хабр", &[], &[Segment("хабр"), Segment("habr")], Profile::of(OnePicture), Planned),
    // AI chats.
    row("chatgpt", "ChatGPT", &[Is("chatgpt.exe"), Is("com.openai.chat")], &[Exact("chatgpt")], ai_chat(10), Planned),
    row(
        "claude",
        "Claude",
        &[Is("claude.exe"), Is("com.anthropic.claudefordesktop")],
        &[Segment("claude")],
        ai_chat(20),
        Planned,
    ),
    row("gemini", "Gemini", &[], &[Exact("google gemini"), Exact("gemini")], ai_chat(10), Planned),
    row("deepseek", "DeepSeek", &[], &[Segment("deepseek")], ai_chat(50), Planned),
    row("alice", "Алиса", &[], &[Exact("алиса"), Segment("алиса ai")], Profile::of(OnePicture), Planned),
    row("gigachat", "GigaChat", &[], &[Segment("gigachat")], Profile::of(OnePicture), Planned),
    // Work and documents.
    row("vscode", "VS Code", &[Is("code.exe"), Is("com.microsoft.vscode")], &[], Profile::of(OnePicture), Planned),
    row(
        "cursor",
        "Cursor",
        &[Is("cursor.exe"), Is("com.todesktop.230313mzl4w4u92")],
        &[],
        Profile::of(OnePicture),
        Planned,
    ),
    row("jira", "Jira", &[], &[Segment("jira")], Profile::of(OnePicture), Planned),
    row("confluence", "Confluence", &[], &[Ends("confluence")], Profile::of(OnePicture), Planned),
    row("notion", "Notion", &[Is("notion.exe"), Is("notion.id")], &[], Profile::of(Sequential), Planned),
    row(
        "gdocs",
        "Google Docs",
        &[],
        &[Segment("google docs"), Segment("google документы")],
        Profile::of(Sequential),
        Planned,
    ),
    row("word", "Word", &[Is("winword.exe"), Is("com.microsoft.word")], &[], Profile::of(Sequential), Planned),
    // Browsers: rules by engine, for pages no row above recognises.
    row(GECKO_ENGINE, "Firefox", &[], &[], Profile::of(OnePicture), Planned),
    // Terminals and editors.
    row(
        "text_editor",
        "Notepad",
        &[Is("notepad.exe"), Is("notepad++.exe"), Is("com.apple.textedit")],
        &[],
        Profile::of(TextOnly),
        Planned,
    ),
    // A multi-line paste into a console may run its lines, so nothing is pasted there.
    row(
        "terminal",
        "Terminal",
        &[
            Is("windowsterminal.exe"),
            Is("cmd.exe"),
            Is("powershell.exe"),
            Is("pwsh.exe"),
            Is("conhost.exe"),
            Is("openconsole.exe"),
            Is("wezterm-gui.exe"),
            Is("alacritty.exe"),
            Is("mintty.exe"),
            Is("com.apple.terminal"),
            Is("com.googlecode.iterm2"),
            Is("dev.warp.warp-stable"),
            Is("com.mitchellh.ghostty"),
        ],
        &[],
        Profile::of(Manual),
        Active,
    ),
    // Everything else: the text, then all snapshots as files.
    row(UNKNOWN, "", &[], &[], Profile::DEFAULT, Active),
];

/// Browsers by executable (lowercase) and, for a generic name, a lowercase fragment of its path.
const BROWSERS: &[(&str, &str, Engine)] = &[
    ("chrome.exe", "", Engine::Chromium),
    ("msedge.exe", "", Engine::Chromium),
    ("browser.exe", r"\yandex\yandexbrowser\", Engine::Chromium),
    ("opera.exe", "", Engine::Chromium),
    ("brave.exe", "", Engine::Chromium),
    ("vivaldi.exe", "", Engine::Chromium),
    ("chromium.exe", "", Engine::Chromium),
    ("arc.exe", "", Engine::Chromium),
    ("firefox.exe", "", Engine::Gecko),
    ("librewolf.exe", "", Engine::Gecko),
    ("waterfox.exe", "", Engine::Gecko),
    ("floorp.exe", "", Engine::Gecko),
    ("zen.exe", "", Engine::Gecko),
    ("com.google.chrome", "", Engine::Chromium),
    ("com.microsoft.edgemac", "", Engine::Chromium),
    ("ru.yandex.desktop.yandex-browser", "", Engine::Chromium),
    ("com.operasoftware.opera", "", Engine::Chromium),
    ("com.brave.browser", "", Engine::Chromium),
    ("com.vivaldi.vivaldi", "", Engine::Chromium),
    ("company.thebrowser.browser", "", Engine::Chromium),
    ("org.mozilla.firefox", "", Engine::Gecko),
    ("com.apple.safari", "", Engine::WebKit),
];

/// Removed before matching: Edge writes U+200B into "Microsoft Edge", Gemini starts its title with U+200E.
const INVISIBLE: [char; 4] = ['\u{200b}', '\u{200e}', '\u{200f}', '\u{feff}'];

/// The browser's own part of a Windows title, lowercase, longest first where one ends another. macOS window
/// names have none; installed web apps (PWA) have none either.
const BROWSER_SUFFIXES: &[&str] = &[
    " - google chrome",
    " - microsoft edge",
    " \u{2014} mozilla firefox private browsing",
    " \u{2014} mozilla firefox",
    " - mozilla firefox",
    " - opera",
    " - brave",
    " - vivaldi",
    " - chromium",
    " - яндекс браузер",
    " - yandex browser",
];

/// Separators between the parts of a page title ("Inbox - Gmail", "Feed | LinkedIn", "Home • Threads").
const SEPARATORS: &[&str] = &[" - ", " | ", " / ", " \u{2022} ", " \u{00b7} ", " \u{2014} ", " \u{2013} "];

pub fn by_id(id: &str) -> Option<&'static Target> {
    REGISTRY.iter().find(|target| target.id == id)
}

fn unknown() -> &'static Target {
    by_id(UNKNOWN).expect("the registry has the unknown row")
}

/// The page engine when `app` is a browser. Both are compared lowercase.
pub fn browser_engine(app: &str, path: &str) -> Option<Engine> {
    let (app, path) = (app.to_lowercase(), path.to_lowercase());
    BROWSERS
        .iter()
        .find(|(name, fragment, _)| app == *name && path.contains(fragment))
        .map(|&(_, _, engine)| engine)
}

/// Only browser titles are read: other programs are recognised by their executable, and their titles (a chat
/// name, a file name) would only add risk.
pub fn is_browser(app: &str, path: &str) -> bool {
    browser_engine(app, path).is_some()
}

pub fn detect(fg: &ForegroundInfo) -> Detected {
    if let Some(engine) = browser_engine(&fg.app, &fg.path) {
        let target = fg.title.as_deref().map(normalize_title).and_then(|title| match_title(&title));
        return Detected { target: target.unwrap_or_else(unknown), engine };
    }
    let app = fg.app.to_lowercase();
    let target = REGISTRY.iter().find(|target| target.apps.iter().any(|rule| rule.matches(&app)));
    Detected { target: target.unwrap_or_else(unknown), engine: Engine::Native }
}

/// The target and the profile to paste with. An unknown or unusable target pastes as an unknown program,
/// unless the browser engine has a usable rule of its own. In a Gecko browser a usable target that expects a
/// file list gets one joined picture instead, keeping its caption limit.
pub fn effective(detected: &Detected, allow_planned: bool) -> (&'static Target, Profile) {
    let usable = |target: &Target| allow_planned || target.status != Status::Planned;
    let gecko = by_id(GECKO_ENGINE).filter(|rule| detected.engine == Engine::Gecko && usable(rule));
    match (Some(detected.target).filter(|t| t.id != UNKNOWN && usable(t)), gecko) {
        (Some(target), Some(_)) => (target, first_file_only(target.profile)),
        (Some(target), None) => (target, target.profile),
        (None, Some(rule)) => (rule, rule.profile),
        (None, None) => (unknown(), unknown().profile),
    }
}

fn first_file_only(profile: Profile) -> Profile {
    match profile.class {
        Class::Default | Class::Album | Class::Attach => {
            Profile { caption_max: profile.caption_max, ..Profile::of(Class::OnePicture) }
        }
        _ => profile,
    }
}

/// Lowercase title without invisible marks, an unread counter such as "(3) " and the browser's suffix.
pub fn normalize_title(raw: &str) -> String {
    let visible: String = raw.chars().filter(|c| !INVISIBLE.contains(c)).collect();
    let lower = visible.trim().to_lowercase();
    let title = strip_counter(&lower);
    let title = BROWSER_SUFFIXES.iter().find_map(|suffix| title.strip_suffix(suffix)).unwrap_or(title);
    title.trim().to_string()
}

fn strip_counter(title: &str) -> &str {
    let Some(rest) = title.strip_prefix('(') else { return title };
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    match rest[digits..].strip_prefix(')') {
        Some(after) if digits > 0 => after.trim_start(),
        _ => title,
    }
}

/// Whole-title rules first. Then the segments from the right, because a site names itself at the end
/// ("Claude - Google Docs" is a document, not the Claude chat).
fn match_title(title: &str) -> Option<&'static Target> {
    if title.is_empty() {
        return None;
    }
    let whole = REGISTRY.iter().find(|target| {
        target.titles.iter().any(|rule| match *rule {
            Exact(text) => title == text,
            Ends(text) => title.ends_with(text),
            Contains(text) => title.contains(text),
            Segment(_) => false,
        })
    });
    whole.or_else(|| {
        segments(title).into_iter().rev().find_map(|segment| {
            REGISTRY.iter().find(|target| target.titles.iter().any(|rule| matches!(*rule, Segment(s) if s == segment)))
        })
    })
}

fn segments(title: &str) -> Vec<&str> {
    let mut parts = vec![title];
    for separator in SEPARATORS {
        parts = parts.into_iter().flat_map(|part| part.split(separator)).collect();
    }
    parts.into_iter().map(str::trim).filter(|part| !part.is_empty()).collect()
}

impl App {
    fn matches(&self, app: &str) -> bool {
        match *self {
            Is(name) => app == name,
            Has(part) => app.contains(part),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paste::plan::{Shape, shape};

    fn native(app: &str) -> &'static str {
        detect(&ForegroundInfo { app: app.into(), ..Default::default() }).target.id
    }

    fn in_browser(app: &str, title: &str) -> Detected {
        detect(&ForegroundInfo { app: app.into(), path: String::new(), title: Some(title.into()) })
    }

    fn chrome(title: &str) -> &'static str {
        in_browser("chrome.exe", title).target.id
    }

    #[test]
    fn programs_by_executable_or_bundle_id() {
        assert_eq!(native("Telegram.exe"), "tg_desktop");
        assert_eq!(native("ru.keepcoder.Telegram"), "tg_desktop");
        assert_eq!(native("org.telegram.desktop"), "tg_desktop");
        assert_eq!(native("AyuGram.exe"), "tg_fork");
        assert_eq!(native("WhatsApp.Root.exe"), "whatsapp");
        assert_eq!(native("Code.exe"), "vscode");
        assert_eq!(native("notepad.exe"), "text_editor");
        assert_eq!(native("WindowsTerminal.exe"), "terminal");
        assert_eq!(native("com.apple.Terminal"), "terminal");
        assert_eq!(native("OUTLOOK.EXE"), "outlook_classic");
        assert_eq!(native("olk.exe"), "outlook_new");
        assert_eq!(native("MailClient.exe"), "emclient");
        assert_eq!(native("applicationframehost.exe"), UNKNOWN);
        assert_eq!(native("wellsnap.exe"), UNKNOWN);
        assert_eq!(native(""), UNKNOWN);
        let detected = detect(&ForegroundInfo { app: "Telegram.exe".into(), ..Default::default() });
        assert_eq!(detected.engine, Engine::Native);
    }

    #[test]
    fn yandex_browser_is_told_apart_by_its_path() {
        let yandex = r"c:\users\me\appdata\local\yandex\yandexbrowser\application\browser.exe";
        assert_eq!(browser_engine("browser.exe", yandex), Some(Engine::Chromium));
        assert_eq!(browser_engine("Browser.exe", &yandex.to_uppercase()), Some(Engine::Chromium));
        assert_eq!(browser_engine("browser.exe", r"c:\program files\other\browser.exe"), None);
        let page = |path: &str| {
            detect(&ForegroundInfo { app: "browser.exe".into(), path: path.into(), title: Some("Telegram Web".into()) })
                .target
                .id
        };
        assert_eq!(page(yandex), "tg_web_k");
        assert_eq!(page(r"c:\program files\other\browser.exe"), UNKNOWN, "titles of other programs are ignored");
        assert!(is_browser("firefox.exe", ""));
        assert!(!is_browser("telegram.exe", ""));
    }

    #[test]
    fn pages_by_browser_title() {
        assert_eq!(chrome("Telegram Web - Google Chrome"), "tg_web_k");
        assert_eq!(chrome("(3) Telegram Web - Google Chrome"), "tg_web_k");
        assert_eq!(chrome("Telegram - Google Chrome"), "tg_web_a");
        assert_eq!(chrome("\u{200e}Google Gemini - Google Chrome"), "gemini");
        let edge = in_browser("msedge.exe", "Inbox (3) - x - Gmail - Personal - Microsoft\u{200b} Edge");
        assert_eq!(edge.target.id, "gmail");
        assert_eq!(chrome("Home / X - Google Chrome"), "x");
        assert_eq!(chrome("(2) Home / X - Google Chrome"), "x");
        assert_eq!(chrome("TELEGRAM WEB - Google Chrome"), "tg_web_k", "case does not matter");
        assert_eq!(chrome("Входящие \u{2014} Яндекс Почта - Google Chrome"), "yandex_mail");
        assert_eq!(chrome("Мессенджер"), "vk_messages", "an installed web app has no suffix");
        assert_eq!(chrome("Новости | ВКонтакте - Google Chrome"), "vk_social");
        assert_eq!(chrome("Feed | LinkedIn - Google Chrome"), "linkedin");
        assert_eq!(chrome("Home \u{2022} Threads - Google Chrome"), "threads");
        assert_eq!(chrome("Статья / Хабр - Google Chrome"), "habr");
        assert_eq!(chrome("Chat title - Claude - Google Chrome"), "claude");
        assert_eq!(chrome("(1) WhatsApp - Google Chrome"), "whatsapp");
        assert_eq!(chrome("Mail - Ivan - Outlook - Google Chrome"), "outlook_web");
        assert_eq!(chrome("Notes - Google Docs - Google Chrome"), "gdocs");
    }

    #[test]
    fn the_site_name_at_the_end_wins() {
        assert_eq!(chrome("Claude - Google Docs - Google Chrome"), "gdocs");
        assert_eq!(chrome("Gmail - Google Docs - Google Chrome"), "gdocs");
        assert_eq!(chrome("Telegram tips - Google Docs - Google Chrome"), "gdocs");
    }

    #[test]
    fn unknown_pages_keep_the_engine() {
        let firefox = in_browser("firefox.exe", "Page \u{2014} Mozilla Firefox");
        assert_eq!((firefox.target.id, firefox.engine), (UNKNOWN, Engine::Gecko));
        let safari = detect(&ForegroundInfo { app: "com.apple.Safari".into(), ..Default::default() });
        assert_eq!((safari.target.id, safari.engine), (UNKNOWN, Engine::WebKit), "no title");
        let empty = in_browser("chrome.exe", "  \u{200b} ");
        assert_eq!((empty.target.id, empty.engine), (UNKNOWN, Engine::Chromium));
        assert_eq!(in_browser("firefox.exe", "Telegram Web \u{2014} Mozilla Firefox").target.id, "tg_web_k");
    }

    #[test]
    fn titles_of_other_programs_are_ignored() {
        let titled =
            |app: &str, title: &str| ForegroundInfo { app: app.into(), path: String::new(), title: Some(title.into()) };
        assert_eq!(detect(&titled("notepad.exe", "Telegram Web")).target.id, "text_editor");
        assert_eq!(detect(&titled("someapp.exe", "Gmail")).target.id, UNKNOWN);
    }

    #[test]
    fn title_normalisation() {
        assert_eq!(normalize_title(" (12) Telegram Web - Google Chrome "), "telegram web");
        assert_eq!(normalize_title("(x) Page"), "(x) page", "not a counter");
        assert_eq!(normalize_title("() Page"), "() page");
        assert_eq!(normalize_title("\u{feff}Page \u{2014} Mozilla Firefox Private Browsing"), "page");
        assert_eq!(normalize_title("Page - Microsoft\u{200b} Edge"), "page");
        assert_eq!(normalize_title("Почта - Яндекс Браузер"), "почта");
        assert_eq!(normalize_title(""), "");
    }

    #[test]
    fn planned_rows_paste_as_unknown_in_release() {
        let gmail = in_browser("chrome.exe", "Inbox - Gmail - Google Chrome");
        assert_eq!(effective(&gmail, true).0.id, "gmail");
        assert_eq!(effective(&gmail, true).1.class, Class::Sequential);
        let (target, profile) = effective(&gmail, false);
        assert_eq!((target.id, profile), (UNKNOWN, Profile::DEFAULT));

        let telegram = detect(&ForegroundInfo { app: "telegram.exe".into(), ..Default::default() });
        let (target, profile) = effective(&telegram, false);
        assert_eq!((target.id, profile.class, profile.caption_max), ("tg_desktop", Class::Album, Some(1024)));
        let terminal = detect(&ForegroundInfo { app: "pwsh.exe".into(), ..Default::default() });
        assert_eq!(effective(&terminal, false).1.class, Class::Manual);
    }

    #[test]
    fn gecko_engine_rule() {
        let page = in_browser("firefox.exe", "Page \u{2014} Mozilla Firefox");
        let (target, profile) = effective(&page, true);
        assert_eq!((target.id, profile.class), (GECKO_ENGINE, Class::OnePicture));
        assert_eq!(effective(&page, false).0.id, UNKNOWN, "planned until checked live");
        let chrome_page = in_browser("chrome.exe", "Page - Google Chrome");
        assert_eq!(effective(&chrome_page, true).0.id, UNKNOWN);

        // A known page in Firefox would receive only the first file of a list.
        let telegram = in_browser("firefox.exe", "Telegram Web \u{2014} Mozilla Firefox");
        let (target, profile) = effective(&telegram, true);
        assert_eq!((target.id, profile.class, profile.caption_max), ("tg_web_k", Class::OnePicture, Some(1024)));
        let gmail = in_browser("firefox.exe", "Inbox - Gmail \u{2014} Mozilla Firefox");
        assert_eq!(effective(&gmail, true).1.class, Class::Sequential, "bitmaps paste fine in Firefox");
        let letters: Vec<char> = "ABC".chars().collect();
        assert_eq!(shape(&effective(&telegram, true).1, &letters, true, false), Shape::Picture);
    }

    #[test]
    fn registry_integrity() {
        let mut ids: Vec<&str> = REGISTRY.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "ids are unique");
        for target in REGISTRY {
            let id_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_';
            assert!(target.id.chars().all(id_char), "{}", target.id);
            let rules = target.apps.iter().map(|rule| match *rule {
                Is(text) | Has(text) => text,
            });
            let titles = target.titles.iter().map(|rule| match *rule {
                Exact(text) | Segment(text) | Ends(text) | Contains(text) => text,
            });
            for text in rules.chain(titles) {
                assert_eq!(text, text.to_lowercase(), "{}: rules are lowercase", target.id);
                assert!(!text.trim().is_empty(), "{}", target.id);
            }
            if target.id != UNKNOWN {
                assert!(!target.name.0.is_empty() && !target.name.1.is_empty(), "{}", target.id);
            }
            if let Status::Verified { date, version } = target.status {
                assert!(date.len() == 10 && !version.is_empty(), "{}", target.id);
            }
            if target.profile.legend || target.profile.max_items.is_some() {
                assert_eq!(target.profile.class, Class::Attach, "{}", target.id);
            }
        }
        assert!(by_id(UNKNOWN).is_some() && by_id(GECKO_ENGINE).is_some());
        assert_eq!(by_id("tg_desktop").unwrap().status, Status::Active, "today's Telegram rule keeps shipping");
        assert_eq!(by_id("terminal").unwrap().status, Status::Active);
        for (name, _, _) in BROWSERS {
            assert_eq!(native(name), UNKNOWN, "{name}: a browser exe is no native target");
        }
    }

    #[test]
    fn names_by_language() {
        let vk = by_id("vk_social").unwrap();
        assert_eq!((vk.name(Lang::Ru), vk.name(Lang::En)), ("ВКонтакте", "VK"));
        let claude = by_id("claude").unwrap();
        assert_eq!((claude.name(Lang::Ru), claude.name(Lang::En)), ("Claude", "Claude"));
    }

    /// The public matrix lists every registry row exactly once per language, with the same status, and contains
    /// no long or en dashes (like every user-visible text).
    #[test]
    fn matrix_matches_registry() {
        let docs = [
            (
                "docs/paste-targets.md",
                include_str!("../../../docs/paste-targets.md"),
                ("Verified", "On, not verified yet", "Not verified"),
            ),
            (
                "docs/paste-targets.ru.md",
                include_str!("../../../docs/paste-targets.ru.md"),
                ("Проверено", "Включено, ещё не проверено", "Не проверено"),
            ),
        ];
        for (file, text, (verified, active, planned)) in docs {
            let mut rows: Vec<(&str, &str)> = Vec::new();
            for line in text.lines().filter(|line| line.trim_start().starts_with('|')) {
                let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
                let Some(id) = cells.last().and_then(|cell| cell.strip_prefix('`')?.strip_suffix('`')) else {
                    continue;
                };
                rows.push((id, cells[cells.len() - 2]));
            }
            for (id, _) in &rows {
                assert!(by_id(id).is_some(), "{file}: row `{id}` is not in the registry");
            }
            for target in REGISTRY {
                let found: Vec<&str> =
                    rows.iter().filter(|(id, _)| *id == target.id).map(|&(_, status)| status).collect();
                assert_eq!(found.len(), 1, "{file}: `{}` must have exactly one row", target.id);
                let expected = match target.status {
                    Status::Verified { date, version } => format!("{verified}: {date}, {version}"),
                    Status::Active => active.to_string(),
                    Status::Planned => planned.to_string(),
                };
                assert_eq!(found[0], expected, "{file}: status of `{}`", target.id);
            }
            assert_no_long_dashes(file, text);
        }
        let template = include_str!("../../../.github/ISSUE_TEMPLATE/paste-target.yml");
        assert_no_long_dashes("paste-target.yml", template);
    }

    fn assert_no_long_dashes(file: &str, text: &str) {
        for (number, line) in text.lines().enumerate() {
            assert!(
                !line.chars().any(|c| ('\u{2012}'..='\u{2015}').contains(&c)),
                "{file}:{}: long or en dash",
                number + 1
            );
        }
    }
}
