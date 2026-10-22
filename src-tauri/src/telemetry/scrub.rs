//! Removes what could name the user or the machine from everything a report carries: the home folder and the
//! custom save folder, the user name and host name, `\Users\<name>\`, `/Users/<name>/` and `/home/<name>/`
//! segments, the organization in `OneDrive - <name>`, UNC server names and screenshot file names. Other folder
//! names stay (install and system paths are what a crash report needs). Runs in `before_send` (in both
//! processes), `before_breadcrumb`, on log lines and on JavaScript messages and stacks. No regex dependency: a
//! few small scanners over chars.
//!
//! The minidump itself is binary and is not touched here; its module list carries the install path, which a
//! Sentry server-side rule ("Usernames in filepaths" on `$minidump.code_file`) removes.

use sentry::protocol::{Breadcrumb, DebugImage, Event, Stacktrace, User, Value};
use std::sync::{Mutex, OnceLock};

/// Longest message, exception value or breadcrumb kept.
pub const MAX_TEXT: usize = 500;
/// Longest JavaScript stack read.
pub const MAX_STACK: usize = 8 * 1024;

const USER: &str = "<user>";
const HOME: &str = "<home>";
const HOST: &str = "<host>";
const FILE: &str = "<file>";
const SAVE_DIR: &str = "<save_dir>";
const ORG: &str = "<org>";

pub struct Scrubber {
    /// The home folder split into its path components, so `\`, `/` and doubled `\\` (Debug output) match.
    home: Option<Vec<Vec<char>>>,
    user: Option<Vec<char>>,
    host: Option<Vec<char>>,
    /// The custom save folder (Settings), which can name a client or a project; changes at runtime.
    save_dir: Mutex<Option<Vec<Vec<char>>>>,
}

/// A folder as path components; None for a drive root or less.
fn path_parts(path: &str) -> Option<Vec<Vec<char>>> {
    let parts: Vec<Vec<char>> = path.split(is_sep).filter(|c| !c.is_empty()).map(|c| c.chars().collect()).collect();
    (parts.len() >= 2).then_some(parts)
}

impl Scrubber {
    /// Names shorter than 3 characters would replace parts of ordinary words.
    pub fn new(home: Option<&str>, user: Option<&str>, host: Option<&str>) -> Self {
        let long = |v: &str| v.trim().chars().count() >= 3;
        Scrubber {
            home: home.and_then(path_parts),
            user: user.filter(|v| long(v)).map(|v| v.trim().chars().collect()),
            host: host.filter(|v| long(v)).map(|v| v.trim().chars().collect()),
            save_dir: Mutex::new(None),
        }
    }

    pub fn set_save_dir(&self, dir: Option<&str>) {
        *self.save_dir.lock().unwrap_or_else(|e| e.into_inner()) = dir.map(str::trim).and_then(path_parts);
    }

    /// The running user's names, read once.
    pub fn global() -> &'static Scrubber {
        static SCRUBBER: OnceLock<Scrubber> = OnceLock::new();
        SCRUBBER.get_or_init(|| {
            let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
            let home = if cfg!(windows) { var("USERPROFILE") } else { var("HOME") };
            let user = var("USERNAME").or_else(|| var("USER"));
            let host = sentry::integrations::contexts::utils::server_name().or_else(|| var("COMPUTERNAME"));
            Scrubber::new(home.as_deref(), user.as_deref(), host.as_deref())
        })
    }

    /// Everything identifying replaced; the length is kept as it is.
    pub fn clean(&self, input: &str) -> String {
        let mut s: Vec<char> = input.chars().collect();
        // First: the save folder is often inside the home folder.
        let save_dir = self.save_dir.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(save_dir) = &save_dir {
            s = replace_path(&s, save_dir, SAVE_DIR);
        }
        if let Some(home) = &self.home {
            s = replace_path(&s, home, HOME);
        }
        s = user_segments(&s);
        s = onedrive_orgs(&s);
        s = unc_hosts(&s);
        if let Some(user) = &self.user {
            s = replace_word(&s, user, USER);
        }
        if let Some(host) = &self.host {
            s = replace_word(&s, host, HOST);
        }
        image_names(&s).into_iter().collect()
    }

    /// `clean` and cut to `MAX_TEXT` characters. A long input is cut before cleaning too, with room for what the
    /// replacements shorten, so a huge message (a data: URL in an error) is not scanned in full.
    pub fn text(&self, input: &str) -> String {
        truncate(&self.clean(&truncate(input, MAX_TEXT * 4)), MAX_TEXT)
    }

    /// The whole event: texts, frames, breadcrumbs, tags, extras and module paths. The host name and the user
    /// fields other than the random id go; the IP is never set (send_default_pii is off).
    pub fn event(&self, event: &mut Event<'static>) {
        event.server_name = None;
        event.request = None;
        event.modules.clear();
        event.user = event.user.take().map(|u| User { id: u.id, ..Default::default() });
        event.message = event.message.take().map(|m| self.text(&m));
        event.culprit = event.culprit.take().map(|m| self.text(&m));
        event.transaction = event.transaction.take().map(|m| self.text(&m));
        if let Some(entry) = &mut event.logentry {
            entry.message = self.text(&entry.message);
            entry.params.clear();
        }
        for exception in &mut event.exception.values {
            exception.value = exception.value.take().map(|v| self.text(&v));
            self.stacktrace(&mut exception.stacktrace);
            self.stacktrace(&mut exception.raw_stacktrace);
        }
        self.stacktrace(&mut event.stacktrace);
        for thread in &mut event.threads.values {
            thread.name = thread.name.take().map(|n| self.text(&n));
            self.stacktrace(&mut thread.stacktrace);
            self.stacktrace(&mut thread.raw_stacktrace);
        }
        for crumb in &mut event.breadcrumbs.values {
            self.breadcrumb(crumb);
        }
        for value in event.tags.values_mut() {
            *value = self.text(value);
        }
        for value in event.extra.values_mut() {
            self.value(value);
        }
        if !event.debug_meta.images.is_empty() {
            for image in &mut event.debug_meta.to_mut().images {
                match image {
                    DebugImage::Symbolic(image) => {
                        image.name = self.clean(&image.name);
                        image.debug_file = image.debug_file.take().map(|f| self.clean(&f));
                    }
                    DebugImage::Apple(image) => image.name = self.clean(&image.name),
                    DebugImage::Wasm(image) => {
                        image.code_file = self.clean(&image.code_file);
                        image.debug_file = image.debug_file.take().map(|f| self.clean(&f));
                    }
                    DebugImage::Proguard(_) => {}
                }
            }
        }
    }

    pub fn breadcrumb(&self, crumb: &mut Breadcrumb) {
        crumb.message = crumb.message.take().map(|m| self.text(&m));
        for value in crumb.data.values_mut() {
            self.value(value);
        }
    }

    fn stacktrace(&self, stacktrace: &mut Option<Stacktrace>) {
        for frame in stacktrace.iter_mut().flat_map(|s| s.frames.iter_mut()) {
            frame.abs_path = frame.abs_path.take().map(|p| self.clean(&p));
            frame.filename = frame.filename.take().map(|p| self.clean(&p));
            frame.vars.clear();
        }
    }

    fn value(&self, value: &mut Value) {
        match value {
            Value::String(s) => *s = self.text(s),
            Value::Array(items) => items.iter_mut().for_each(|v| self.value(v)),
            Value::Object(map) => map.values_mut().for_each(|v| self.value(v)),
            _ => {}
        }
    }
}

/// At most `max` characters; a cut text ends with "...".
pub fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text.to_string(),
    }
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// Letters and digits continue a name; punctuation and separators end it.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Ends a path segment that is not followed by a separator.
fn ends_segment(c: char) -> bool {
    is_sep(c) || matches!(c, '"' | '\'' | '`' | '\n' | '\r' | '\t' | '<' | '>' | '|' | '*' | '?')
}

fn same(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn matches_at(hay: &[char], at: usize, needle: &[char]) -> bool {
    at + needle.len() <= hay.len() && needle.iter().zip(&hay[at..]).all(|(n, h)| same(*n, *h))
}

fn push_str(out: &mut Vec<char>, s: &str) {
    out.extend(s.chars());
}

/// The end of the folder path starting at `at`, separators of any kind and count between components.
fn path_at(s: &[char], at: usize, parts: &[Vec<char>]) -> Option<usize> {
    let mut j = at;
    for (k, part) in parts.iter().enumerate() {
        if k > 0 {
            if j >= s.len() || !is_sep(s[j]) {
                return None;
            }
            while j < s.len() && is_sep(s[j]) {
                j += 1;
            }
        }
        if !matches_at(s, j, part) {
            return None;
        }
        j += part.len();
    }
    // "C:\Users\Al" must not match inside "C:\Users\Alice".
    (j == s.len() || !is_word(s[j])).then_some(j)
}

fn replace_path(s: &[char], parts: &[Vec<char>], with: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if (i == 0 || !is_word(s[i - 1]))
            && let Some(end) = path_at(s, i, parts)
        {
            push_str(&mut out, with);
            i = end;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// The folder after `\Users\`, `/Users/` or `/home/` (any case, any separators) becomes `<user>`. A name is
/// taken up to the next separator; at the end of a path (no separator follows) only up to a space, so the
/// rest of a log message stays.
fn user_segments(s: &[char]) -> Vec<char> {
    const MARKERS: [&[char]; 2] = [&['u', 's', 'e', 'r', 's'], &['h', 'o', 'm', 'e']];
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if i > 0 && is_sep(s[i - 1]) {
            let marker = MARKERS.iter().find(|m| matches_at(s, i, m));
            if let Some(marker) = marker {
                let after = i + marker.len();
                if after < s.len() && is_sep(s[after]) {
                    let mut start = after;
                    while start < s.len() && is_sep(s[start]) {
                        start += 1;
                    }
                    let mut end = start;
                    while end < s.len() && !ends_segment(s[end]) {
                        end += 1;
                    }
                    if end >= s.len() || !is_sep(s[end]) {
                        end = (start..end).find(|&k| s[k].is_whitespace()).unwrap_or(end);
                    }
                    let name: String = s[start..end].iter().collect();
                    if !name.is_empty() && name != USER && name != HOME {
                        out.extend_from_slice(&s[i..start]);
                        push_str(&mut out, USER);
                        i = end;
                        continue;
                    }
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// A work OneDrive folder is named after the organization: `OneDrive - Contoso Ltd` becomes `OneDrive - <org>`.
/// The name runs to the next separator; at the end of a path (no separator follows) only up to a space, as in
/// `user_segments`.
fn onedrive_orgs(s: &[char]) -> Vec<char> {
    const MARKER: [char; 11] = ['o', 'n', 'e', 'd', 'r', 'i', 'v', 'e', ' ', '-', ' '];
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if (i == 0 || is_sep(s[i - 1])) && matches_at(s, i, &MARKER) {
            let start = i + MARKER.len();
            let mut end = start;
            while end < s.len() && !ends_segment(s[end]) {
                end += 1;
            }
            if end >= s.len() || !is_sep(s[end]) {
                end = (start..end).find(|&k| s[k].is_whitespace()).unwrap_or(end);
            }
            let name: String = s[start..end].iter().collect();
            if !name.is_empty() && name != ORG {
                out.extend_from_slice(&s[i..start]);
                push_str(&mut out, ORG);
                i = end;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `\\server\share` becomes `\\<host>\share`. Only at the start of a token, so the doubled backslashes of a
/// Debug-printed `C:\\Users` are left to the other rules.
fn unc_hosts(s: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let token_start = i == 0 || s[i - 1].is_whitespace() || matches!(s[i - 1], '"' | '\'' | '(' | '[' | '=');
        if token_start && s[i] == '\\' && s.get(i + 1) == Some(&'\\') {
            let mut start = i;
            while start < s.len() && s[start] == '\\' {
                start += 1;
            }
            let mut end = start;
            while end < s.len() && !ends_segment(s[end]) && !s[end].is_whitespace() {
                end += 1;
            }
            let host: String = s[start..end].iter().collect();
            // `\\?\C:\...` and `\\.\pipe\...` are local.
            if !host.is_empty() && host != "?" && host != "." && host != HOST {
                out.extend_from_slice(&s[i..start]);
                push_str(&mut out, HOST);
                i = end;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// Every whole-word occurrence of `word`, ignoring case.
fn replace_word(s: &[char], word: &[char], with: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let end = i + word.len();
        if (i == 0 || !is_word(s[i - 1])) && matches_at(s, i, word) && (end == s.len() || !is_word(s[end])) {
            push_str(&mut out, with);
            i = end;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `C:\shots\Invoice March.png` keeps only `<file>.png`: screenshot names often describe what is on them.
fn image_names(s: &[char]) -> Vec<char> {
    const EXTS: [&[char]; 3] = [&['p', 'n', 'g'], &['j', 'p', 'e', 'g'], &['j', 'p', 'g']];
    let ends_name = |c: char| {
        is_sep(c) || c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '(' | '[' | '<' | '>' | ':' | '=' | ',' | '{')
    };
    let mut names = Vec::new();
    for dot in 0..s.len() {
        if s[dot] != '.' {
            continue;
        }
        let Some(ext) = EXTS.iter().find(|e| matches_at(s, dot + 1, e)) else { continue };
        let after = dot + 1 + ext.len();
        if after < s.len() && is_word(s[after]) {
            continue;
        }
        let mut start = dot;
        while start > 0 && !ends_name(s[start - 1]) {
            start -= 1;
        }
        let name: String = s[start..dot].iter().collect();
        if start < dot && name != FILE {
            names.push((start, dot));
        }
    }
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    for (start, end) in names {
        if start < i {
            continue;
        }
        out.extend_from_slice(&s[i..start]);
        push_str(&mut out, FILE);
        i = end;
    }
    out.extend_from_slice(&s[i..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::{DebugMeta, Exception, SymbolicDebugImage};

    fn windows() -> Scrubber {
        Scrubber::new(Some(r"C:\Users\Анна Петрова"), Some("Анна Петрова"), Some("DESKTOP-7QK2"))
    }

    #[test]
    fn home_folder_with_any_separators_and_case() {
        let s = windows();
        assert_eq!(s.clean(r"open C:\Users\Анна Петрова\AppData\x.log"), r"open <home>\AppData\x.log");
        assert_eq!(s.clean("c:/users/анна петрова/x"), "<home>/x");
        // Debug output doubles the backslashes.
        assert_eq!(s.clean(r#"path: "C:\\Users\\Анна Петрова\\a""#), r#"path: "<home>\\a""#);
        assert_eq!(s.clean("file:///C:/Users/Анна Петрова/a"), "file:///<home>/a");
    }

    #[test]
    fn user_folders_of_other_users_and_onedrive() {
        let s = Scrubber::new(None, None, None);
        assert_eq!(s.clean(r"D:\Users\bob\OneDrive - Contoso\x.txt"), r"D:\Users\<user>\OneDrive - <org>\x.txt");
        assert_eq!(s.clean(r"<home>\OneDrive - Acme Corp Ltd\Docs"), r"<home>\OneDrive - <org>\Docs");
        assert_eq!(s.clean("/Users/x/OneDrive - Acme failed"), "/Users/<user>/OneDrive - <org> failed");
        assert_eq!(s.clean(r"<home>\OneDrive\Docs"), r"<home>\OneDrive\Docs", "a personal OneDrive has no name");
        assert_eq!(s.clean("/Users/alice/Library/Logs"), "/Users/<user>/Library/Logs");
        assert_eq!(s.clean("/home/carol/.config"), "/home/<user>/.config");
        assert_eq!(s.clean(r"C:\\Users\\John Smith\\Desktop"), r"C:\\Users\\<user>\\Desktop");
        // At the end of a path only the name goes, not the rest of the message.
        assert_eq!(s.clean("missing /home/dave failed"), "missing /home/<user> failed");
        assert_eq!(s.clean("C:/Users/eve"), "C:/Users/<user>");
        assert_eq!(s.clean("no path users here"), "no path users here");
    }

    #[test]
    fn unc_server_names() {
        let s = Scrubber::new(None, None, None);
        assert_eq!(s.clean(r"copy to \\FILESRV01\share\a.txt"), r"copy to \\<host>\share\a.txt");
        assert_eq!(s.clean(r#""\\nas\x""#), r#""\\<host>\x""#);
        assert_eq!(s.clean(r"\\?\C:\Program Files"), r"\\?\C:\Program Files");
        assert_eq!(s.clean(r"\\.\pipe\x"), r"\\.\pipe\x");
    }

    #[test]
    fn user_and_host_names_as_whole_words() {
        let s = windows();
        assert_eq!(s.clean("hello АННА ПЕТРОВА on desktop-7qk2"), "hello <user> on <host>");
        let short = Scrubber::new(None, Some("al"), Some("pc"));
        assert_eq!(short.clean("al on pc"), "al on pc", "names under 3 characters are not replaced");
        let word = Scrubber::new(None, Some("max"), None);
        assert_eq!(word.clean("maximum max"), "maximum <user>");
    }

    #[test]
    fn screenshot_file_names() {
        let s = Scrubber::new(None, None, None);
        assert_eq!(s.clean(r"saved D:\shots\Invoice-March.PNG ok"), r"saved D:\shots\<file>.PNG ok");
        assert_eq!(s.clean("a.jpg, b.jpeg"), "<file>.jpg, <file>.jpeg");
        assert_eq!(s.clean("x.pngs and .png"), "x.pngs and .png");
        assert_eq!(s.clean("app:///assets/logo.png"), "app:///assets/<file>.png");
    }

    #[test]
    fn the_custom_save_folder() {
        let s = windows();
        s.set_save_dir(Some(r"D:\Clients\Acme Corp"));
        assert_eq!(s.clean(r"saved to D:\Clients\Acme Corp\Invoice.png"), r"saved to <save_dir>\<file>.png");
        assert_eq!(s.clean(r#"detail: Some("D:\\Clients\\Acme Corp")"#), r#"detail: Some("<save_dir>")"#);
        assert_eq!(s.clean(r"D:\Clients\Acme Corporation"), r"D:\Clients\Acme Corporation", "a whole folder only");
        // Inside the home folder: the save folder goes first and whole.
        s.set_save_dir(Some(r"C:\Users\Анна Петрова\Projects\X"));
        assert_eq!(s.clean(r"C:\Users\Анна Петрова\Projects\X\a"), r"<save_dir>\a");
        assert_eq!(s.clean(r"C:\Users\Анна Петрова\Projects\Y"), r"<home>\Projects\Y");
        s.set_save_dir(Some(r"D:\"));
        assert_eq!(s.clean(r"D:\x"), r"D:\x", "a drive root is not a name");
        s.set_save_dir(None);
        assert_eq!(s.clean(r"D:\Clients\Acme Corp"), r"D:\Clients\Acme Corp");
    }

    #[test]
    fn texts_are_cut() {
        assert_eq!(truncate("абв", 2), "аб...");
        assert_eq!(truncate("абв", 3), "абв");
        let long = "x".repeat(MAX_TEXT + 10);
        assert_eq!(Scrubber::new(None, None, None).text(&long).chars().count(), MAX_TEXT + 3);
        // A huge input is cut before cleaning and still scrubbed where it is kept.
        let huge = format!(r"C:\Users\anna\a {}", "y".repeat(1_000_000));
        let text = Scrubber::new(None, None, None).text(&huge);
        assert!(text.starts_with(r"C:\Users\<user>\a yyy") && text.chars().count() == MAX_TEXT + 3, "{text}");
    }

    #[test]
    fn events_lose_names_paths_and_the_host() {
        let s = windows();
        let mut event = Event {
            server_name: Some("DESKTOP-7QK2".into()),
            message: Some(r"failed at C:\Users\Анна Петрова\x".into()),
            user: Some(User { id: Some("crash-id".into()), username: Some("anna".into()), ..Default::default() }),
            exception: vec![Exception {
                ty: "panic".into(),
                value: Some("on DESKTOP-7QK2".into()),
                ..Default::default()
            }]
            .into(),
            debug_meta: std::borrow::Cow::Owned(DebugMeta {
                images: vec![DebugImage::Symbolic(SymbolicDebugImage {
                    name: r"C:\Users\Анна Петрова\AppData\Local\WellSnap\WellSnap.exe".into(),
                    arch: None,
                    image_addr: 0.into(),
                    image_size: 0,
                    image_vmaddr: 0.into(),
                    id: Default::default(),
                    code_id: None,
                    debug_file: Some(r"C:\Users\Анна Петрова\AppData\Local\WellSnap\WellSnap.pdb".into()),
                })],
                ..Default::default()
            }),
            ..Default::default()
        };
        s.event(&mut event);
        assert_eq!(event.server_name, None);
        assert_eq!(event.message.as_deref(), Some(r"failed at <home>\x"));
        assert_eq!(event.exception.values[0].value.as_deref(), Some("on <host>"));
        let user = event.user.unwrap();
        assert_eq!((user.id.as_deref(), user.username), (Some("crash-id"), None));
        let DebugImage::Symbolic(image) = &event.debug_meta.images[0] else { panic!() };
        assert_eq!(image.name, r"<home>\AppData\Local\WellSnap\WellSnap.exe");
        assert_eq!(image.debug_file.as_deref(), Some(r"<home>\AppData\Local\WellSnap\WellSnap.pdb"));
    }
}
