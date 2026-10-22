//! JavaScript errors from the webviews, turned into Sentry events here instead of loading a Sentry SDK into
//! every page. The stack is parsed from V8 (WebView2) and JavaScriptCore (WKWebView) text; frames of the app's
//! own scripts are rewritten to `app:///assets/...`, where the uploaded source maps live, and every other
//! frame is dropped. The script debug ids become source map images (see outbox.rs, `DEBUG_IDS_EXTRA`).

use super::scrub::{self, Scrubber};
use super::{JsErrorKind, JsErrorReport};
use sentry::protocol::{Event, Exception, Frame, Level, Mechanism, Stacktrace, Value};
use std::collections::BTreeMap;

/// Frames kept per stack, innermost first.
const MAX_FRAMES: usize = 50;
/// Extra key carrying `abs_path -> debug id` to the transport, which turns it into `debug_meta.images`.
pub const DEBUG_IDS_EXTRA: &str = "wellsnap.debug_ids";

#[derive(Debug, Clone, PartialEq)]
pub struct JsFrame {
    pub function: Option<String>,
    pub abs_path: String,
    pub line: u64,
    pub column: Option<u64>,
}

/// The page's own scripts: `http(s)://tauri.localhost/...` (Windows) or `tauri://localhost/...` (macOS),
/// without query and fragment.
pub fn app_url(url: &str) -> Option<String> {
    let path = ["http://tauri.localhost/", "https://tauri.localhost/", "tauri://localhost/"]
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix))?;
    let path = path.split(['?', '#']).next().unwrap_or("");
    (!path.is_empty()).then(|| format!("app:///{path}"))
}

/// Innermost frame first, as both engines print them; only frames in the app's own scripts.
pub fn parse_stack(stack: &str) -> Vec<JsFrame> {
    stack.lines().filter_map(parse_line).take(MAX_FRAMES).collect()
}

fn parse_line(line: &str) -> Option<JsFrame> {
    let line = line.trim();
    let (function, location) = if let Some(rest) = line.strip_prefix("at ") {
        // V8: "at fn (url:line:col)", "at url:line:col", "at async fn (url:line:col)".
        match rest.strip_suffix(')').and_then(|r| r.rsplit_once(" (")) {
            Some((function, location)) => (Some(function.trim_start_matches("async ").trim()), location),
            None => (None, rest.trim_start_matches("async ")),
        }
    } else if let Some((function, location)) = line.split_once('@') {
        // JavaScriptCore: "fn@url:line:col", "@url:line:col".
        (Some(function), location)
    } else {
        (None, line)
    };
    let (url, line, column) = split_location(location)?;
    let abs_path = app_url(url)?;
    let function = function.filter(|f| !f.is_empty()).map(str::to_string);
    Some(JsFrame { function, abs_path, line, column })
}

/// "url:line:col" or "url:line".
fn split_location(location: &str) -> Option<(&str, u64, Option<u64>)> {
    let (head, last) = location.rsplit_once(':')?;
    let last: u64 = last.parse().ok()?;
    match head.rsplit_once(':').and_then(|(url, line)| line.parse::<u64>().ok().map(|line| (url, line))) {
        Some((url, line)) => Some((url, line, Some(last))),
        None => Some((head, last, None)),
    }
}

/// "TypeError: x is not a function" gives ("TypeError", "x is not a function").
fn split_message(message: &str, kind: JsErrorKind) -> (String, String) {
    let fallback = match kind {
        JsErrorKind::Error => "Error",
        JsErrorKind::UnhandledRejection => "UnhandledRejection",
    };
    match message.split_once(": ") {
        Some((ty, value)) if !ty.is_empty() && ty.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
            (ty.to_string(), value.to_string())
        }
        _ => (fallback.to_string(), message.to_string()),
    }
}

/// Overlay windows are numbered per monitor; they are one page.
pub fn page_name(label: &str) -> &str {
    if crate::overlay::is_overlay_label(label) { "overlay" } else { label }
}

/// The event and its rate-limit fingerprint.
pub fn event(page: &str, report: &JsErrorReport, scrubber: &Scrubber) -> (Event<'static>, String) {
    let stack = report.stack.as_deref().map(|s| scrub::truncate(s, scrub::MAX_STACK)).unwrap_or_default();
    let frames = parse_stack(&stack);
    let (ty, value) = split_message(&report.message, report.kind);
    let value = scrubber.text(&value);
    let page = page_name(page).to_string();
    let top = frames.first().map(|f| format!("{}:{}:{}", f.abs_path, f.line, f.column.unwrap_or(0)));
    let fingerprint = format!("js|{page}|{ty}|{}", top.unwrap_or_else(|| value.clone()));

    let debug_ids: BTreeMap<String, Value> = report
        .debug_ids
        .iter()
        .filter_map(|(url, id)| Some((app_url(url)?, id)))
        .filter(|(path, _)| frames.iter().any(|f| &f.abs_path == path))
        .map(|(path, id)| (path, Value::String(id.clone())))
        .collect();
    let sentry_frames: Vec<Frame> = frames
        .iter()
        .rev()
        .map(|f| Frame {
            function: f.function.as_deref().map(|name| scrubber.text(name)),
            abs_path: Some(f.abs_path.clone()),
            filename: f.abs_path.strip_prefix("app://").map(str::to_string),
            lineno: Some(f.line),
            colno: f.column,
            in_app: Some(true),
            ..Default::default()
        })
        .collect();
    let mechanism = match report.kind {
        JsErrorKind::Error => "onerror",
        JsErrorKind::UnhandledRejection => "onunhandledrejection",
    };
    let mut event = Event {
        level: Level::Error,
        platform: "javascript".into(),
        exception: vec![Exception {
            ty,
            value: Some(value),
            stacktrace: (!sentry_frames.is_empty()).then(|| Stacktrace { frames: sentry_frames, ..Default::default() }),
            mechanism: Some(Mechanism { ty: mechanism.into(), handled: Some(false), ..Default::default() }),
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    };
    event.tags.insert("page".into(), page);
    event.tags.insert("source".into(), "js".into());
    if !debug_ids.is_empty() {
        event.extra.insert(DEBUG_IDS_EXTRA.into(), Value::Object(debug_ids.into_iter().collect()));
    }
    (event, fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const V8: &str = "TypeError: Cannot read properties of null (reading 'x')
    at draw (http://tauri.localhost/assets/editor-Ab12.js:1:2345)
    at http://tauri.localhost/assets/index-Cd34.js?v=1:7:89
    at async save (http://tauri.localhost/assets/editor-Ab12.js:3:4)
    at Array.map (<anonymous>)
    at https://evil.example/x.js:1:1";

    const JSC: &str = "draw@tauri://localhost/assets/editor-Ab12.js:1:2345
@tauri://localhost/assets/index-Cd34.js:7:89
promiseReactionJob@[native code]
global code@tauri://localhost/assets/main.js:10";

    #[test]
    fn v8_frames_of_the_app_only() {
        let frames = parse_stack(V8);
        assert_eq!(
            frames,
            vec![
                JsFrame {
                    function: Some("draw".into()),
                    abs_path: "app:///assets/editor-Ab12.js".into(),
                    line: 1,
                    column: Some(2345)
                },
                JsFrame { function: None, abs_path: "app:///assets/index-Cd34.js".into(), line: 7, column: Some(89) },
                JsFrame {
                    function: Some("save".into()),
                    abs_path: "app:///assets/editor-Ab12.js".into(),
                    line: 3,
                    column: Some(4)
                },
            ]
        );
    }

    #[test]
    fn jsc_frames_of_the_app_only() {
        let frames = parse_stack(JSC);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].function.as_deref(), Some("draw"));
        assert_eq!(frames[1].function, None);
        assert_eq!((frames[1].line, frames[1].column), (7, Some(89)));
        assert_eq!(frames[2], JsFrame {
            function: Some("global code".into()),
            abs_path: "app:///assets/main.js".into(),
            line: 10,
            column: None
        });
    }

    #[test]
    fn only_tauri_urls_become_app_urls() {
        assert_eq!(app_url("http://tauri.localhost/assets/a.js").as_deref(), Some("app:///assets/a.js"));
        assert_eq!(app_url("tauri://localhost/assets/a.js#x").as_deref(), Some("app:///assets/a.js"));
        assert_eq!(app_url("http://127.0.0.1:1420/src/main.tsx"), None, "dev server");
        assert_eq!(app_url("https://tauri.localhost.evil.com/a.js"), None);
        assert_eq!(app_url("http://tauri.localhost/"), None);
    }

    #[test]
    fn event_with_source_map_ids_and_scrubbed_message() {
        let scrubber = Scrubber::new(Some(r"C:\Users\anna"), Some("anna"), None);
        let report = JsErrorReport {
            kind: JsErrorKind::Error,
            message: r"TypeError: cannot open C:\Users\anna\shot.png".into(),
            stack: Some(V8.into()),
            debug_ids: HashMap::from([
                ("http://tauri.localhost/assets/editor-Ab12.js".to_string(), "id-editor".to_string()),
                ("http://tauri.localhost/assets/unused.js".to_string(), "id-unused".to_string()),
            ]),
        };
        let (event, fingerprint) = event("overlay-2", &report, &scrubber);
        assert_eq!(event.platform, "javascript");
        assert_eq!(event.tags["page"], "overlay");
        let exception = &event.exception.values[0];
        assert_eq!(exception.ty, "TypeError");
        assert_eq!(exception.value.as_deref(), Some(r"cannot open <home>\<file>.png"));
        let frames = &exception.stacktrace.as_ref().unwrap().frames;
        assert_eq!(frames.last().unwrap().function.as_deref(), Some("draw"), "outermost first");
        assert_eq!(frames.last().unwrap().filename.as_deref(), Some("/assets/editor-Ab12.js"));
        assert_eq!(
            event.extra[DEBUG_IDS_EXTRA],
            serde_json::json!({"app:///assets/editor-Ab12.js": "id-editor"}),
            "only scripts that appear in the stack"
        );
        assert_eq!(fingerprint, "js|overlay|TypeError|app:///assets/editor-Ab12.js:1:2345");
    }

    #[test]
    fn a_rejection_without_stack() {
        let report = JsErrorReport {
            kind: JsErrorKind::UnhandledRejection,
            message: "not an Error: just text".into(),
            stack: None,
            debug_ids: HashMap::new(),
        };
        let (event, fingerprint) = event("settings", &report, &Scrubber::new(None, None, None));
        let exception = &event.exception.values[0];
        assert_eq!(exception.ty, "UnhandledRejection");
        assert_eq!(exception.value.as_deref(), Some("not an Error: just text"));
        assert!(exception.stacktrace.is_none());
        assert_eq!(exception.mechanism.as_ref().unwrap().ty, "onunhandledrejection");
        assert!(!event.extra.contains_key(DEBUG_IDS_EXTRA));
        assert_eq!(fingerprint, "js|settings|UnhandledRejection|not an Error: just text");
    }
}
