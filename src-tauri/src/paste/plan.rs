//! Pure planning for paste with text: which pictures the target receives (`shape`) and in which order the text
//! and the pictures are pasted (`plan`). Building the pictures, the clipboard and the key presses stay in
//! actions.rs; the target and its profile come from target.rs.
#![allow(dead_code)] // Until WP4b moves smart paste in actions.rs onto these functions.

use crate::i18n::{Lang, tr};

/// Most pictures pasted one by one: every step repeats the pause and the focus check, so 10 take about 7.5 s.
pub const SEQUENTIAL_MAX: usize = 10;

/// How a target takes text and pictures. `Profile` adds the target's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Unknown program: the text, then all snapshots as files (the behaviour before targets existed).
    Default,
    /// Telegram: the text typed first becomes the album caption, within the caption and album limits.
    Album,
    /// Chats and AI chats: the text, then all snapshots as attachments in one paste; above `max_items` the
    /// snapshots are joined into that many pictures.
    Attach,
    /// Takes one picture per paste: the text, then every snapshot joined into one picture.
    OnePicture,
    /// Mail and editors that read HTML: one paste of the full "Copy all" payload (pictures inline, comments
    /// under them). Without embedded pictures (too large), as `OnePicture`.
    RichHtml,
    /// Inline editors (web mail, Teams, documents): the text, then every snapshot as its own bitmap paste.
    Sequential,
    /// Plain text editors: only the text.
    TextOnly,
    /// Terminals: no key is sent; the text goes to the clipboard and a notice explains why.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub class: Class,
    /// Most attachments the target takes in one message (`Attach`).
    pub max_items: Option<usize>,
    /// Longer text does not fit as a caption: the pictures go first and the text stays in the clipboard.
    pub caption_max: Option<usize>,
    /// More pictures than one album holds: the same as a too long caption.
    pub album_max: Option<usize>,
    /// A line in front of the text tells which picture shows which snapshot (AI chats refer to pictures by
    /// position).
    pub legend: bool,
}

impl Profile {
    pub const DEFAULT: Profile = Profile::of(Class::Default);

    pub const fn of(class: Class) -> Profile {
        Profile { class, max_items: None, caption_max: None, album_max: None, legend: false }
    }
}

/// What the target receives. Letters are the snapshot letters of the set, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// All snapshot files in one paste.
    Files,
    /// One paste of a file list with one picture per group; a group of one keeps its own file.
    Grouped(Vec<Vec<char>>),
    /// One bitmap paste per snapshot.
    Each,
    /// One bitmap paste per group.
    EachGrouped(Vec<Vec<char>>),
    /// One paste of the "Copy all" payload: files, text and HTML with the pictures embedded.
    CopyAll,
    /// One bitmap of every snapshot joined.
    Picture,
    /// No pictures.
    None,
}

impl Shape {
    /// Clipboard contents to paste one after another, `Step::Content(0..n)`.
    pub fn contents(&self, snapshots: usize) -> usize {
        match self {
            Shape::Files | Shape::Grouped(_) | Shape::CopyAll | Shape::Picture => 1,
            Shape::Each => snapshots,
            Shape::EachGrouped(groups) => groups.len(),
            Shape::None => 0,
        }
    }

    /// Pictures that arrive in the target.
    pub fn images(&self, snapshots: usize) -> usize {
        match self {
            Shape::Files | Shape::Each | Shape::CopyAll => snapshots,
            Shape::Grouped(groups) | Shape::EachGrouped(groups) => groups.len(),
            Shape::Picture => 1,
            Shape::None => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Put the text on the clipboard and paste it.
    Text,
    /// Put content `k` on the clipboard and paste it.
    Content(usize),
    /// Leave the text on the clipboard for the user; no key press.
    TextToClipboard,
}

/// Splits the letters into at most `max_groups` contiguous groups whose sizes differ by at most one, the larger
/// ones first. Fewer letters than groups gives one letter per group, so as few pictures as possible are joined.
pub fn groups(letters: &[char], max_groups: usize) -> Vec<Vec<char>> {
    let count = letters.len().min(max_groups.max(1));
    if count == 0 {
        return Vec::new();
    }
    let (base, extra) = (letters.len() / count, letters.len() % count);
    let mut rest = letters;
    (0..count)
        .map(|i| {
            let (group, tail) = rest.split_at(base + usize::from(i < extra));
            rest = tail;
            group.to_vec()
        })
        .collect()
}

/// What the target receives for a snapshot set. One-picture mode (the strip toggle) changes only the shape:
/// the target's caption, text-only and manual rules still apply.
pub fn shape(profile: &Profile, letters: &[char], html_has_images: bool, one_picture_mode: bool) -> Shape {
    if letters.is_empty() || matches!(profile.class, Class::TextOnly | Class::Manual) {
        return Shape::None;
    }
    if one_picture_mode {
        return Shape::Picture;
    }
    match profile.class {
        Class::Default | Class::Album => Shape::Files,
        Class::Attach => match profile.max_items {
            Some(max) if letters.len() > max => Shape::Grouped(groups(letters, max)),
            _ => Shape::Files,
        },
        Class::OnePicture => Shape::Picture,
        Class::RichHtml if html_has_images => Shape::CopyAll,
        Class::RichHtml => Shape::Picture,
        Class::Sequential if letters.len() > SEQUENTIAL_MAX => Shape::EachGrouped(groups(letters, SEQUENTIAL_MAX)),
        Class::Sequential => Shape::Each,
        Class::TextOnly | Class::Manual => Shape::None,
    }
}

/// The snapshots each received picture shows, in paste order: the legend and the notices are built from it.
pub fn pictures(shape: &Shape, letters: &[char]) -> Vec<Vec<char>> {
    match shape {
        Shape::Files | Shape::Each | Shape::CopyAll => letters.iter().map(|&letter| vec![letter]).collect(),
        Shape::Grouped(groups) | Shape::EachGrouped(groups) => groups.clone(),
        Shape::Picture if !letters.is_empty() => vec![letters.to_vec()],
        Shape::Picture | Shape::None => Vec::new(),
    }
}

/// The paste steps. `snapshots` is the number of snapshots behind `shape` (1 for a single copy, which is a
/// `Shape::Picture`). Empty text never gives a `Text` step; smart paste never presses Enter.
pub fn plan(text: &str, shape: &Shape, snapshots: usize, profile: &Profile) -> Vec<Step> {
    let has_text = !text.is_empty();
    match profile.class {
        Class::TextOnly => return if has_text { vec![Step::Text] } else { Vec::new() },
        Class::Manual => return if has_text { vec![Step::TextToClipboard] } else { Vec::new() },
        _ => {}
    }
    let content: Vec<Step> = (0..shape.contents(snapshots)).map(Step::Content).collect();
    // The "Copy all" payload carries the text itself.
    if !has_text || *shape == Shape::CopyAll {
        return content;
    }
    let too_long = profile.caption_max.is_some_and(|max| text.chars().count() > max);
    let too_many = profile.album_max.is_some_and(|max| shape.images(snapshots) > max);
    if too_long || too_many {
        content.into_iter().chain([Step::TextToClipboard]).collect()
    } else {
        [Step::Text].into_iter().chain(content).collect()
    }
}

/// «Картинки по порядку: 1) Снимок A; 2) Снимки B, C».
pub fn legend(lang: Lang, groups: &[Vec<char>]) -> String {
    let items: Vec<String> = groups
        .iter()
        .enumerate()
        .map(|(i, group)| {
            let title = tr(lang, if group.len() == 1 { "block.snapshot" } else { "block.snapshots" });
            let letters: Vec<String> = group.iter().map(char::to_string).collect();
            format!("{}) {title} {}", i + 1, letters.join(", "))
        })
        .collect();
    format!("{} {}", tr(lang, "block.legend"), items.join("; "))
}

/// The text to paste: with the legend line in front when the target asks for it, there is text and at least two
/// pictures arrive.
pub fn text_with_legend(lang: Lang, text: &str, profile: &Profile, pictures: &[Vec<char>]) -> String {
    if profile.legend && !text.is_empty() && pictures.len() >= 2 {
        format!("{}\n\n{text}", legend(lang, pictures))
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TELEGRAM: Profile =
        Profile { class: Class::Album, max_items: None, caption_max: Some(1024), album_max: Some(10), legend: false };
    const CLAUDE: Profile = Profile { max_items: Some(20), legend: true, ..Profile::of(Class::Attach) };

    fn letters(n: usize) -> Vec<char> {
        (0..n).map(|i| (b'A' + i as u8) as char).collect()
    }

    fn sizes(groups: &[Vec<char>]) -> Vec<usize> {
        groups.iter().map(Vec::len).collect()
    }

    fn content(n: usize) -> Vec<Step> {
        (0..n).map(Step::Content).collect()
    }

    #[test]
    fn groups_are_contiguous_and_balanced() {
        let all = letters(26);
        let to_claude = groups(&all, 20);
        assert_eq!(sizes(&to_claude), [vec![2; 6], vec![1; 14]].concat());
        let to_ten = groups(&all, 10);
        assert_eq!(sizes(&to_ten), [vec![3; 6], vec![2; 4]].concat());
        for split in [&to_claude, &to_ten] {
            assert_eq!(split.concat(), all, "every letter once, in order");
        }
        assert_eq!(groups(&letters(5), 4), vec![vec!['A', 'B'], vec!['C'], vec!['D'], vec!['E']]);
        assert_eq!(groups(&letters(3), 10), vec![vec!['A'], vec!['B'], vec!['C']]);
        assert_eq!(groups(&letters(3), 0), vec![letters(3)], "at least one group");
        assert!(groups(&[], 4).is_empty());
    }

    #[test]
    fn shape_per_class() {
        let all = letters(26);
        assert_eq!(shape(&TELEGRAM, &all, true, false), Shape::Files);
        assert_eq!(shape(&Profile::DEFAULT, &all, true, false), Shape::Files);
        assert_eq!(shape(&CLAUDE, &letters(20), true, false), Shape::Files);
        assert_eq!(shape(&CLAUDE, &all, true, false), Shape::Grouped(groups(&all, 20)));
        let x = Profile { max_items: Some(4), ..Profile::of(Class::Attach) };
        let Shape::Grouped(to_x) = shape(&x, &letters(5), false, false) else { panic!("grouped") };
        assert_eq!(sizes(&to_x), [2, 1, 1, 1]);
        assert_eq!(shape(&Profile::of(Class::Attach), &all, false, false), Shape::Files, "no limit");
        assert_eq!(shape(&Profile::of(Class::OnePicture), &all, true, false), Shape::Picture);
        assert_eq!(shape(&Profile::of(Class::RichHtml), &all, true, false), Shape::CopyAll);
        assert_eq!(shape(&Profile::of(Class::RichHtml), &all, false, false), Shape::Picture);
        assert_eq!(shape(&Profile::of(Class::Sequential), &letters(10), false, false), Shape::Each);
        let Shape::EachGrouped(each) = shape(&Profile::of(Class::Sequential), &letters(11), false, false) else {
            panic!("each grouped")
        };
        assert_eq!(each.len(), SEQUENTIAL_MAX);
        assert_eq!(shape(&Profile::of(Class::TextOnly), &all, true, false), Shape::None);
        assert_eq!(shape(&Profile::of(Class::Manual), &all, true, false), Shape::None);
        assert_eq!(shape(&TELEGRAM, &[], true, false), Shape::None);
    }

    #[test]
    fn one_picture_mode_changes_only_the_shape() {
        let all = letters(12);
        let rich = Profile::of(Class::RichHtml);
        for profile in [TELEGRAM, CLAUDE, Profile::DEFAULT, rich, Profile::of(Class::Sequential)] {
            assert_eq!(shape(&profile, &all, true, true), Shape::Picture, "{profile:?}");
        }
        assert_eq!(shape(&Profile::of(Class::Manual), &all, true, true), Shape::None);
        assert_eq!(shape(&Profile::of(Class::TextOnly), &all, true, true), Shape::None);
        // Telegram keeps its caption rule; one picture is never over the album limit.
        let long = "я".repeat(1025);
        assert_eq!(plan(&long, &Shape::Picture, 12, &TELEGRAM), vec![Step::Content(0), Step::TextToClipboard]);
        assert_eq!(plan("A1 x", &Shape::Picture, 12, &TELEGRAM), vec![Step::Text, Step::Content(0)]);
    }

    /// The Telegram and unknown-program plans before targets existed (actions.rs `plan`), case by case.
    #[test]
    fn telegram_and_default_keep_the_old_plans() {
        let files = Shape::Files;
        assert_eq!(plan("", &files, 3, &TELEGRAM), content(1));
        assert_eq!(plan("A1 x", &files, 3, &Profile::DEFAULT), vec![Step::Text, Step::Content(0)]);
        assert_eq!(plan("A1 x", &files, 3, &TELEGRAM), vec![Step::Text, Step::Content(0)]);
        assert_eq!(plan("A1 x", &files, 11, &TELEGRAM), vec![Step::Content(0), Step::TextToClipboard]);
        assert_eq!(plan("A1 x", &files, 11, &Profile::DEFAULT), vec![Step::Text, Step::Content(0)]);
        let long = "я".repeat(1025);
        assert_eq!(plan(&long, &Shape::Picture, 1, &TELEGRAM), vec![Step::Content(0), Step::TextToClipboard]);
        assert_eq!(plan(&"я".repeat(1024), &Shape::Picture, 1, &TELEGRAM), vec![Step::Text, Step::Content(0)]);
        assert_eq!(plan(&long, &files, 26, &Profile::DEFAULT), vec![Step::Text, Step::Content(0)]);
    }

    #[test]
    fn plan_per_class() {
        let all = letters(12);
        let attach = shape(&CLAUDE, &all, true, false);
        assert_eq!(plan("A1 x", &attach, 12, &CLAUDE), vec![Step::Text, Step::Content(0)]);
        let grouped = shape(&CLAUDE, &letters(26), true, false);
        assert_eq!(plan("A1 x", &grouped, 26, &CLAUDE), vec![Step::Text, Step::Content(0)], "one file list");

        let sequential = Profile::of(Class::Sequential);
        let each = shape(&sequential, &letters(4), true, false);
        assert_eq!(plan("A1 x", &each, 4, &sequential), [vec![Step::Text], content(4)].concat());
        let each = shape(&sequential, &letters(26), true, false);
        assert_eq!(plan("A1 x", &each, 26, &sequential), [vec![Step::Text], content(10)].concat());

        let rich = Profile::of(Class::RichHtml);
        assert_eq!(plan("A1 x", &Shape::CopyAll, 12, &rich), content(1), "the payload carries the text");
        assert_eq!(plan("A1 x", &Shape::Picture, 12, &rich), vec![Step::Text, Step::Content(0)], "too large");

        let one = Profile::of(Class::OnePicture);
        assert_eq!(plan("A1 x", &Shape::Picture, 12, &one), vec![Step::Text, Step::Content(0)]);

        let text_only = Profile::of(Class::TextOnly);
        assert_eq!(plan("A1 x", &Shape::None, 12, &text_only), vec![Step::Text]);
        assert_eq!(plan("A1 x", &Shape::Picture, 1, &text_only), vec![Step::Text], "a single copy too");
        let manual = Profile::of(Class::Manual);
        assert_eq!(plan("A1 x", &Shape::None, 12, &manual), vec![Step::TextToClipboard]);
    }

    #[test]
    fn empty_text_never_gives_a_text_step() {
        let all = letters(12);
        let classes = [
            Class::Default,
            Class::Album,
            Class::Attach,
            Class::OnePicture,
            Class::RichHtml,
            Class::Sequential,
            Class::TextOnly,
            Class::Manual,
        ];
        for class in classes {
            let limits = Profile { caption_max: Some(1), album_max: Some(1), max_items: Some(4), ..Profile::DEFAULT };
            let profile = Profile { class, ..limits };
            for html in [true, false] {
                let shape = shape(&profile, &all, html, false);
                let steps = plan("", &shape, all.len(), &profile);
                assert!(steps.iter().all(|s| matches!(s, Step::Content(_))), "{class:?}: {steps:?}");
                assert_eq!(steps.len(), shape.contents(all.len()), "{class:?}");
            }
        }
    }

    #[test]
    fn pictures_follow_the_shape() {
        let all = letters(3);
        assert_eq!(pictures(&Shape::Files, &all), vec![vec!['A'], vec!['B'], vec!['C']]);
        assert_eq!(pictures(&Shape::Each, &all), pictures(&Shape::CopyAll, &all));
        assert_eq!(pictures(&Shape::Picture, &all), vec![all.clone()]);
        let grouped = vec![vec!['A', 'B'], vec!['C']];
        assert_eq!(pictures(&Shape::Grouped(grouped.clone()), &all), grouped);
        assert_eq!(pictures(&Shape::EachGrouped(grouped.clone()), &all), grouped);
        assert!(pictures(&Shape::None, &all).is_empty());
        assert!(pictures(&Shape::Picture, &[]).is_empty());
        for shape in [Shape::Files, Shape::Each, Shape::CopyAll, Shape::Picture, Shape::Grouped(grouped)] {
            assert_eq!(pictures(&shape, &all).len(), shape.images(all.len()), "{shape:?}");
        }
    }

    #[test]
    fn legend_in_both_languages() {
        let groups = vec![vec!['A'], vec!['B', 'C']];
        assert_eq!(legend(Lang::Ru, &groups), "Картинки по порядку: 1) Снимок A; 2) Снимки B, C");
        assert_eq!(legend(Lang::En, &groups), "Images in order: 1) Screenshot A; 2) Screenshots B, C");
        assert_eq!(
            legend(Lang::En, &[vec!['A'], vec!['C'], vec!['D']]),
            "Images in order: 1) Screenshot A; 2) Screenshot C; 3) Screenshot D"
        );
    }

    #[test]
    fn legend_only_where_the_target_asks_for_it() {
        let two = vec![vec!['A'], vec!['B']];
        assert_eq!(
            text_with_legend(Lang::En, "A1 x", &CLAUDE, &two),
            "Images in order: 1) Screenshot A; 2) Screenshot B\n\nA1 x"
        );
        assert_eq!(text_with_legend(Lang::En, "", &CLAUDE, &two), "", "no text, no legend");
        assert_eq!(text_with_legend(Lang::En, "A1 x", &CLAUDE, &[vec!['A', 'B']]), "A1 x", "one picture");
        assert_eq!(text_with_legend(Lang::En, "A1 x", &Profile::of(Class::Attach), &two), "A1 x");
    }
}
