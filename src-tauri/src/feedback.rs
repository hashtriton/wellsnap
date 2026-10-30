//! "Write to the author" in Settings: one POST to the site, which mails it to the owner
//! (wellsnap_site, public/api/feedback.php). Not telemetry: it goes out only when the user presses Send, in every
//! build, and carries what the form names: the text, an optional reply address, the version, the OS and the UI
//! language.

use crate::i18n::Lang;
use serde::Serialize;
use std::sync::OnceLock;
use std::time::Duration;

const ENDPOINT: &str = "https://wellsnap.net/api/feedback.php";
const TIMEOUT: Duration = Duration::from_secs(30);
/// Same limit as the endpoint, in characters.
pub const MESSAGE_MAX: usize = 5000;

/// What the form shows for a failed send.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackError {
    Empty,
    TooLong,
    BadEmail,
    RateLimited,
    /// The site's total for the day is used up.
    DailyLimit,
    Offline,
    Failed,
}

#[derive(Serialize)]
struct Payload<'a> {
    message: &'a str,
    email: Option<&'a str>,
    version: &'a str,
    os: &'a str,
    lang: &'a str,
}

/// The same checks the endpoint makes, so the common mistakes are shown without a round trip. The address check
/// is loose on purpose: the endpoint has the final word.
fn prepare<'a>(message: &'a str, email: &'a str) -> Result<(&'a str, Option<&'a str>), FeedbackError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(FeedbackError::Empty);
    }
    if message.chars().count() > MESSAGE_MAX {
        return Err(FeedbackError::TooLong);
    }
    let email = email.trim();
    if email.is_empty() {
        return Ok((message, None));
    }
    let plausible = email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && email.split_once('@').is_some_and(|(user, host)| !user.is_empty() && host.contains('.'));
    if plausible { Ok((message, Some(email))) } else { Err(FeedbackError::BadEmail) }
}

/// The endpoint replies with `{"ok":true}` or `{"error":"<code>"}` and a matching status. A 2xx without `ok` is
/// not taken as sent: a broken script can still answer 200.
fn outcome(status: u16, body: Option<&serde_json::Value>) -> Result<(), FeedbackError> {
    let error = body.and_then(|b| b.get("error")).and_then(|e| e.as_str());
    let ok = body.and_then(|b| b.get("ok")).and_then(|v| v.as_bool()) == Some(true);
    match (status, error) {
        (200..=299, _) if ok => Ok(()),
        (429, Some("daily_limit")) => Err(FeedbackError::DailyLimit),
        (429, _) => Err(FeedbackError::RateLimited),
        (413, _) | (400, Some("too_long")) => Err(FeedbackError::TooLong),
        (400, Some("bad_email")) => Err(FeedbackError::BadEmail),
        (400, Some("empty")) => Err(FeedbackError::Empty),
        _ => Err(FeedbackError::Failed),
    }
}

pub async fn send(version: &str, lang: Lang, message: &str, email: &str) -> Result<(), FeedbackError> {
    let (message, email) = prepare(message, email)?;
    // reqwest is built with `rustls-no-provider`, as for the updater and telemetry. Installing it twice is harmless.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        // A redirect would turn the POST into a GET without the message.
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("wellsnap/{version}"))
        .build()
        .map_err(|err| {
            log::warn!("feedback: no http client: {err}");
            FeedbackError::Failed
        })?;
    let payload = Payload {
        message,
        email,
        version,
        os: os_label(),
        lang: if lang == Lang::Ru { "ru" } else { "en" },
    };
    let response = client.post(ENDPOINT).json(&payload).send().await.map_err(|err| {
        log::warn!("feedback: not sent: {err}");
        if err.is_connect() || err.is_timeout() { FeedbackError::Offline } else { FeedbackError::Failed }
    })?;
    let status = response.status().as_u16();
    let body = response.json::<serde_json::Value>().await.ok();
    let result = outcome(status, body.as_ref());
    if result.is_err() {
        log::warn!("feedback: endpoint answered {status} {}", body.map(|b| b.to_string()).unwrap_or_default());
    }
    result
}

/// "Windows 11 (26100)" or "macOS 15.1": shown in the form and sent with the message.
pub fn os_label() -> &'static str {
    static LABEL: OnceLock<String> = OnceLock::new();
    LABEL.get_or_init(detect_os)
}

#[cfg(windows)]
fn detect_os() -> String {
    windows_label(crate::printscreen::windows_build())
}

#[cfg(target_os = "macos")]
fn detect_os() -> String {
    std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.len() <= 16 && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .map_or_else(|| "macOS".to_string(), |v| format!("macOS {v}"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn detect_os() -> String {
    "other".to_string()
}

#[cfg(any(windows, test))]
fn windows_label(build: Option<u32>) -> String {
    match build {
        Some(build) => format!("Windows {} ({build})", if build >= 22000 { 11 } else { 10 }),
        None => "Windows".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_trims_and_checks_the_text() {
        assert_eq!(prepare("  hi \n", ""), Ok(("hi", None)));
        assert_eq!(prepare(" \n\t", "a@b.c"), Err(FeedbackError::Empty));
        let at_limit = "я".repeat(MESSAGE_MAX);
        assert!(prepare(&at_limit, "").is_ok());
        assert_eq!(prepare(&format!("{at_limit}я"), ""), Err(FeedbackError::TooLong));
    }

    #[test]
    fn prepare_checks_the_address_loosely() {
        assert_eq!(prepare("hi", " user@example.com "), Ok(("hi", Some("user@example.com"))));
        assert_eq!(prepare("hi", "   "), Ok(("hi", None)));
        for bad in ["nope", "@example.com", "user@localhost", "a b@c.d", "a@b.c\r\nBcc: x@y.z"] {
            assert_eq!(prepare("hi", bad), Err(FeedbackError::BadEmail), "{bad}");
        }
        assert_eq!(prepare("hi", &format!("{}@b.cc", "a".repeat(250))), Err(FeedbackError::BadEmail));
    }

    #[test]
    fn outcome_maps_the_endpoint_replies() {
        let reply = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        let error = |code: &str| serde_json::json!({ "error": code });
        assert_eq!(outcome(200, Some(&reply(r#"{"ok":true}"#))), Ok(()));
        assert_eq!(outcome(200, None), Err(FeedbackError::Failed));
        assert_eq!(outcome(200, Some(&reply(r#"{"ok":false}"#))), Err(FeedbackError::Failed));
        assert_eq!(outcome(429, Some(&error("rate_limited"))), Err(FeedbackError::RateLimited));
        assert_eq!(outcome(429, None), Err(FeedbackError::RateLimited));
        assert_eq!(outcome(429, Some(&error("daily_limit"))), Err(FeedbackError::DailyLimit));
        assert_eq!(outcome(413, Some(&error("too_long"))), Err(FeedbackError::TooLong));
        assert_eq!(outcome(400, Some(&error("too_long"))), Err(FeedbackError::TooLong));
        assert_eq!(outcome(400, Some(&error("bad_email"))), Err(FeedbackError::BadEmail));
        assert_eq!(outcome(400, Some(&error("empty"))), Err(FeedbackError::Empty));
        assert_eq!(outcome(400, Some(&error("invalid"))), Err(FeedbackError::Failed));
        assert_eq!(outcome(403, None), Err(FeedbackError::Failed));
        assert_eq!(outcome(502, Some(&error("mail"))), Err(FeedbackError::Failed));
        assert_eq!(outcome(301, None), Err(FeedbackError::Failed));
    }

    #[test]
    fn windows_label_names_the_release() {
        assert_eq!(windows_label(Some(26100)), "Windows 11 (26100)");
        assert_eq!(windows_label(Some(22000)), "Windows 11 (22000)");
        assert_eq!(windows_label(Some(19045)), "Windows 10 (19045)");
        assert_eq!(windows_label(None), "Windows");
    }

    #[test]
    fn os_label_fits_the_endpoint_pattern() {
        let label = os_label();
        assert!(label.len() <= 64);
        assert!(label.chars().all(|c| c.is_ascii_alphanumeric() || " .()_-".contains(c)), "{label}");
    }
}
