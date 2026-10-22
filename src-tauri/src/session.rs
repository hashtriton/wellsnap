//! A snapshot set: screenshots A..Z with numbered marks, stored as PNG files so they can be put on the
//! clipboard as a file list (one Ctrl+V gives an album in Telegram and attachments in web chats).

use crate::i18n::{Lang, tr};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const MAX_SNAPSHOTS: usize = 26;
const MAX_COMMENT_CHARS: usize = 500;
/// Pictures are embedded in the clipboard HTML only while the whole set stays under this size (as base64):
/// 26 4K screenshots would make the HTML hundreds of megabytes.
const HTML_IMAGES_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Session folders older than this are removed (at startup and hourly while running).
pub const MAX_FOLDER_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub num: u32,
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub letter: char,
    #[serde(skip)]
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub marks: Vec<Mark>,
    /// The comments are drawn on the image, so the text block must not repeat them.
    pub comments_on_image: bool,
    /// Bumped when the file changes so thumbnails are not served from cache.
    pub revision: u32,
    /// Physical pixels per CSS pixel of the monitor it was taken on, so snapshots from mixed-DPI monitors can
    /// be drawn at one visual size.
    pub scale: f32,
}

impl Snapshot {
    /// Marks whose comment travels as text (empty when the comments are on the image).
    fn text_marks(&self) -> impl Iterator<Item = &Mark> {
        self.marks.iter().filter(|m| !self.comments_on_image && !m.comment.is_empty())
    }

    /// Some comment of this snapshot travels as text.
    pub fn has_text(&self) -> bool {
        self.text_marks().next().is_some()
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub dir: PathBuf,
    pub snapshots: Vec<Snapshot>,
    /// Letters are never reused inside a session: removing B keeps C as C, because letters are drawn
    /// into the images.
    letters_used: usize,
    /// Last add, removal or reserved letter: an update may clear a set only after it sat unchanged for a day.
    pub changed_at: SystemTime,
}

impl Session {
    pub fn new(paste_root: &Path, now: SystemTime) -> Self {
        let stamp = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
        let id = format!("s{stamp}");
        Self { dir: paste_root.join(&id), id, snapshots: Vec::new(), letters_used: 0, changed_at: now }
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn next_letter(&self) -> Option<char> {
        (self.letters_used < MAX_SNAPSHOTS).then(|| (b'A' + self.letters_used as u8) as char)
    }

    pub fn add(
        &mut self,
        png: &[u8],
        width: u32,
        height: u32,
        scale: f32,
        marks: Vec<Mark>,
        comments_on_image: bool,
    ) -> std::io::Result<char> {
        let letter = self
            .next_letter()
            .ok_or_else(|| std::io::Error::other("snapshot set is full"))?;
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{letter}.png"));
        write_file_atomic(&path, png)?;
        self.letters_used += 1;
        self.snapshots.push(Snapshot {
            letter,
            path,
            width,
            height,
            marks: clean_marks(marks),
            comments_on_image,
            revision: 0,
            scale,
        });
        self.changed_at = SystemTime::now();
        Ok(letter)
    }

    /// A single copy or save that went out with the next letter keeps it. False when `letter` is not the next
    /// letter.
    pub fn skip_letter(&mut self, letter: char) -> bool {
        if self.next_letter() != Some(letter) {
            return false;
        }
        self.letters_used += 1;
        self.changed_at = SystemTime::now();
        true
    }

    /// Returns the removed snapshot's file, which the caller deletes once a running paste no longer needs it.
    pub fn remove(&mut self, letter: char) -> Option<PathBuf> {
        let index = self.snapshots.iter().position(|s| s.letter == letter)?;
        self.changed_at = SystemTime::now();
        Some(self.snapshots.remove(index).path)
    }

    pub fn get(&self, letter: char) -> Option<&Snapshot> {
        self.snapshots.iter().find(|s| s.letter == letter)
    }

    pub fn files(&self) -> Vec<PathBuf> {
        self.snapshots.iter().map(|s| s.path.clone()).collect()
    }

    /// Letters of the snapshots in the set, in order.
    pub fn letters(&self) -> Vec<char> {
        self.snapshots.iter().map(|s| s.letter).collect()
    }

    /// "Снимок A / A1 comment / A2 comment / (blank) / Снимок B / ...". Only commented marks are listed;
    /// snapshots without text comments (none, or drawn on the image) are omitted. Empty when there is nothing
    /// to say.
    pub fn text_block(&self, lang: Lang) -> String {
        let title = tr(lang, "block.snapshot");
        let mut parts = Vec::new();
        for snap in &self.snapshots {
            let lines: Vec<String> =
                snap.text_marks().map(|m| format!("{}{} {}", snap.letter, m.num, m.comment)).collect();
            if !lines.is_empty() {
                parts.push(format!("{title} {}\n{}", snap.letter, lines.join("\n")));
            }
        }
        parts.join("\n\n")
    }

    /// HTML for rich editors (Word, Outlook, LibreOffice): each picture embedded as a data URL, followed by the
    /// comments. Never a file URL: that would put the cache path, with the user's login, into the document, and
    /// the link breaks once the cache is cleaned. Reads the set's files.
    pub fn html_block(&self, lang: Lang) -> String {
        self.html_with_limit(lang, HTML_IMAGES_MAX_BYTES).0
    }

    /// `html_block` and whether every picture is in it (false when the set is too large to embed them, or a
    /// file could not be read).
    #[allow(dead_code)] // Used by smart paste's rich-HTML targets (WP4b).
    pub fn html_block_with_images(&self, lang: Lang) -> (String, bool) {
        self.html_with_limit(lang, HTML_IMAGES_MAX_BYTES)
    }

    /// Without pictures when they would exceed `limit` bytes as base64; the files still go as a file list.
    fn html_with_limit(&self, lang: Lang, limit: usize) -> (String, bool) {
        let title = tr(lang, "block.snapshot");
        let total: u64 = self.snapshots.iter().map(|s| fs::metadata(&s.path).map_or(0, |m| m.len())).sum();
        let embed = usize::try_from(total).is_ok_and(|total| total.div_ceil(3) * 4 <= limit);
        let mut complete = embed && !self.snapshots.is_empty();
        let mut html = String::new();
        for snap in &self.snapshots {
            html.push_str(&format!("<p><b>{title} {}</b></p>", snap.letter));
            let png = embed.then(|| fs::read(&snap.path).ok()).flatten();
            complete &= png.is_some();
            if let Some(png) = png {
                html.push_str(&format!(
                    "<p><img src=\"data:image/png;base64,{}\" width=\"{}\" height=\"{}\" alt=\"{title} {}\"></p>",
                    base64(&png),
                    snap.width,
                    snap.height,
                    snap.letter
                ));
            }
            let items: Vec<String> = snap
                .text_marks()
                .map(|m| format!("<li><b>{}{}</b> {}</li>", snap.letter, m.num, escape_html(&m.comment)))
                .collect();
            if !items.is_empty() {
                html.push_str(&format!("<ul>{}</ul>", items.join("")));
            }
        }
        (html, complete)
    }
}

/// Text block for a single screenshot that is not part of a set: marks are numbered without a letter.
pub fn single_text_block(marks: &[Mark]) -> String {
    marks
        .iter()
        .filter(|m| !m.comment.is_empty())
        .map(|m| format!("{} {}", m.num, m.comment))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn clean_marks(marks: Vec<Mark>) -> Vec<Mark> {
    marks
        .into_iter()
        .map(|m| Mark { num: m.num, comment: clean_comment(&m.comment) })
        .collect()
}

/// One line per mark in the text block, so line breaks become spaces; very long comments are cut.
pub fn clean_comment(raw: &str) -> String {
    let one_line: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(MAX_COMMENT_CHARS).collect()
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Standard base64 with padding (RFC 4648).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Remove old session folders. Files of a set must outlive the paste (chats upload them asynchronously
/// from disk), so only folders older than `max_age` that are not the current session are deleted.
pub fn cleanup_old(paste_root: &Path, keep: Option<&str>, max_age: Duration) {
    let Ok(entries) = fs::read_dir(paste_root) else { return };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if Some(name.as_str()) == keep {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > max_age);
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(name: &str) -> Session {
        let root = std::env::temp_dir().join(format!("wellsnap-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Session::new(&root, SystemTime::now())
    }

    fn marks(list: &[(u32, &str)]) -> Vec<Mark> {
        list.iter().map(|(n, c)| Mark { num: *n, comment: c.to_string() }).collect()
    }

    #[test]
    fn scale_is_stored_and_sent_to_the_frontend() {
        let mut s = session("scale");
        s.add(b"png", 30, 20, 1.5, vec![], true).unwrap();
        assert_eq!(s.get('A').unwrap().scale, 1.5);
        let json = serde_json::to_value(s.get('A').unwrap()).unwrap();
        assert_eq!(json["scale"], 1.5);
        assert_eq!(json["commentsOnImage"], true);
        assert!(json.get("path").is_none(), "the file path stays in the core");
    }

    #[test]
    fn letters_are_sequential_and_not_reused() {
        let mut s = session("letters");
        assert_eq!(s.add(b"png", 10, 10, 1.0, vec![], false).unwrap(), 'A');
        assert_eq!(s.add(b"png", 10, 10, 1.0, vec![], false).unwrap(), 'B');
        assert!(s.remove('B').is_some_and(|p| p.ends_with("B.png")));
        assert_eq!(s.remove('B'), None);
        assert_eq!(s.add(b"png", 10, 10, 1.0, vec![], false).unwrap(), 'C');
        assert_eq!(s.files().len(), 2);
        assert!(s.files()[1].ends_with("C.png"));
    }

    #[test]
    fn a_letter_used_by_a_single_copy_is_skipped() {
        let mut s = session("skip");
        s.add(b"png", 10, 10, 1.0, vec![], false).unwrap();
        s.add(b"png", 10, 10, 1.0, vec![], false).unwrap();
        assert!(!s.skip_letter('D'), "not the next letter");
        assert!(s.skip_letter('C'));
        assert!(!s.skip_letter('C'), "already used");
        assert_eq!(s.add(b"png", 10, 10, 1.0, vec![], false).unwrap(), 'D');
    }

    #[test]
    fn changes_move_changed_at() {
        let root = std::env::temp_dir().join(format!("wellsnap-test-changed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let mut s = Session::new(&root, start);
        assert_eq!(s.changed_at, start);
        s.add(b"png", 1, 1, 1.0, vec![], false).unwrap();
        assert!(s.changed_at > start);
        s.changed_at = start;
        assert!(!s.skip_letter('Z'), "a refused letter changes nothing");
        assert_eq!(s.changed_at, start);
        assert!(s.skip_letter('B'));
        assert!(s.changed_at > start);
        s.changed_at = start;
        assert_eq!(s.remove('Q'), None);
        assert_eq!(s.changed_at, start);
        assert!(s.remove('A').is_some());
        assert!(s.changed_at > start);
    }

    #[test]
    fn set_is_capped_at_26() {
        let mut s = session("cap");
        for _ in 0..26 {
            s.add(b"png", 1, 1, 1.0, vec![], false).unwrap();
        }
        assert_eq!(s.next_letter(), None);
        assert!(s.add(b"png", 1, 1, 1.0, vec![], false).is_err());
        assert_eq!(s.snapshots.last().unwrap().letter, 'Z');
    }

    #[test]
    fn text_block_matches_the_badges() {
        let mut s = session("text");
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "Кнопку ярче"), (2, "Цену  крупнее\nи жирнее")]), false).unwrap();
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "")]), false).unwrap();
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "Здесь текст налезает")]), false).unwrap();
        assert_eq!(
            s.text_block(Lang::Ru),
            "Снимок A\nA1 Кнопку ярче\nA2 Цену крупнее и жирнее\n\nСнимок C\nC1 Здесь текст налезает"
        );
        assert!(s.text_block(Lang::En).starts_with("Screenshot A\n"));
    }

    #[test]
    fn comments_drawn_on_the_image_are_not_repeated_as_text() {
        let mut s = session("onimage");
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "на картинке")]), true).unwrap();
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "текстом")]), false).unwrap();
        assert_eq!(s.text_block(Lang::Ru), "Снимок B
B1 текстом");
        let html = s.html_block(Lang::Ru);
        assert!(!html.contains("на картинке") && html.contains("текстом"));
    }

    #[test]
    fn empty_text_when_no_comments() {
        let mut s = session("notext");
        s.add(b"png", 1, 1, 1.0, marks(&[(1, ""), (2, "  ")]), false).unwrap();
        assert_eq!(s.text_block(Lang::Ru), "");
    }

    #[test]
    fn single_block_has_no_letters() {
        assert_eq!(single_text_block(&marks(&[(1, "a"), (2, ""), (3, "b")])), "1 a\n3 b");
    }

    #[test]
    fn html_escapes_comments() {
        let mut s = session("html");
        s.add(b"png", 3, 4, 1.0, marks(&[(1, "<b>x</b> & y")]), false).unwrap();
        let html = s.html_block(Lang::En);
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt; &amp; y"));
        assert!(html.contains("width=\"3\" height=\"4\""));
    }

    #[test]
    fn html_embeds_pictures_without_paths() {
        let mut s = session("html-embed");
        s.add(b"png", 3, 4, 1.0, marks(&[(1, "x")]), false).unwrap();
        let html = s.html_block(Lang::En);
        assert!(html.contains("<img src=\"data:image/png;base64,cG5n\""));
        assert!(!html.contains("file:") && !html.contains(&*s.dir.to_string_lossy()));
        // Over the size limit: headings and comments only.
        let (small, embedded) = s.html_with_limit(Lang::En, 3);
        assert!(!small.contains("<img") && small.contains("Screenshot A") && small.contains("A1</b> x"));
        assert!(!embedded, "smart paste must not treat it as rich HTML");
        let (full, embedded) = s.html_with_limit(Lang::En, 4);
        assert!(full.contains("<img") && embedded);
        assert_eq!(s.html_block_with_images(Lang::En), (s.html_block(Lang::En), true));
        assert_eq!(session("html-empty").html_block_with_images(Lang::En), (String::new(), false));
        // A file gone from the cache: the HTML lacks its picture, so it is not complete.
        s.add(b"png", 1, 1, 1.0, vec![], false).unwrap();
        fs::remove_file(&s.get('B').unwrap().path).unwrap();
        let (partial, embedded) = s.html_block_with_images(Lang::En);
        assert!(partial.contains("<img") && partial.contains("Screenshot B") && !embedded);
    }

    #[test]
    fn letters_and_text_follow_the_set() {
        let mut s = session("letters-list");
        assert!(s.letters().is_empty());
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "on image")]), true).unwrap();
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "")]), false).unwrap();
        s.add(b"png", 1, 1, 1.0, marks(&[(1, "text")]), false).unwrap();
        s.remove('B');
        assert_eq!(s.letters(), vec!['A', 'C']);
        assert!(!s.get('A').unwrap().has_text(), "drawn on the image");
        assert!(s.get('C').unwrap().has_text());
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in cases {
            assert_eq!(base64(input.as_bytes()), expected);
        }
        assert_eq!(base64(&[0xFF, 0xEF, 0xBE]), "/+++");
    }

    #[test]
    fn comments_are_single_line_and_capped() {
        assert_eq!(clean_comment("  a\n b\t c  "), "a b c");
        assert_eq!(clean_comment(&"x".repeat(900)).chars().count(), MAX_COMMENT_CHARS);
    }
}
