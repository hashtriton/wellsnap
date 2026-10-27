//! Strings that the Rust side shows itself (tray menu, text blocks put on the clipboard, notices).
//! Everything rendered inside webviews is localized on the frontend (src/locales).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum LanguagePref {
    #[default]
    Auto,
    Ru,
    En,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn resolve(pref: LanguagePref) -> Lang {
        match pref {
            LanguagePref::Ru => Lang::Ru,
            LanguagePref::En => Lang::En,
            LanguagePref::Auto => Self::from_locale(sys_locale::get_locale().as_deref()),
        }
    }

    /// Russian UI for Russian and the closest CIS locales, English otherwise.
    pub fn from_locale(locale: Option<&str>) -> Lang {
        let primary = locale
            .unwrap_or("")
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match primary.as_str() {
            "ru" | "be" | "uk" | "kk" | "ky" | "uz" | "tg" | "hy" | "az" => Lang::Ru,
            _ => Lang::En,
        }
    }
}

pub fn tr(lang: Lang, key: &str) -> &'static str {
    let ru = lang == Lang::Ru;
    match key {
        "tray.capture" => if ru { "Сделать скриншот" } else { "Take screenshot" },
        "tray.strip" => if ru { "Показать ленту снимков" } else { "Show snapshot strip" },
        "tray.closePins" => if ru { "Закрыть закреплённые снимки ({count})" } else { "Close pinned screenshots ({count})" },
        "tray.settings" => if ru { "Настройки..." } else { "Settings..." },
        "tray.support" => if ru { "Поддержать проект" } else { "Support the project" },
        "tray.supported" => if ru { "Спасибо за поддержку" } else { "Thanks for your support" },
        "tray.about" => if ru { "О программе" } else { "About" },
        "tray.quit" => if ru { "Выход" } else { "Quit" },
        "tray.update" => if ru { "Установить обновление {version}" } else { "Install update {version}" },
        "toast.updateFailed" => if ru { "Не удалось установить обновление" } else { "Could not install the update" },
        "toast.updateBusy" => if ru {
            "Идёт снимок или вставка, попробуйте позже"
        } else {
            "A capture or paste is running, try again later"
        },
        "toast.updateGone" => if ru { "Это обновление больше не предлагается" } else { "This update is no longer offered" },
        "tray.tooltip" => "WellSnap",
        "block.snapshot" => if ru { "Снимок" } else { "Screenshot" },
        "toast.copied" => if ru { "Скопировано" } else { "Copied" },
        "toast.copiedAll" => if ru { "Скопированы все снимки" } else { "All screenshots copied" },
        "toast.saved" => if ru { "Сохранено" } else { "Saved" },
        "toast.captureFailed" => if ru { "Не удалось сделать снимок экрана" } else { "Could not capture the screen" },
        "toast.textInClipboard" => if ru {
            "Картинки вставлены. Текст в буфере: вставьте его отдельным сообщением"
        } else {
            "Images pasted. The text is in the clipboard: paste it as a separate message"
        },
        "toast.pasteFailed" => if ru { "Не удалось вставить" } else { "Could not paste" },
        "toast.smartPasteHint" => if ru {
            "{hotkey}: вставить картинку вместе с текстом"
        } else {
            "{hotkey}: paste the picture together with the text"
        },
        "toast.smartPasteOff" => if ru {
            "{hotkey} сейчас недоступно (занято другим приложением), его можно сменить в настройках"
        } else {
            "{hotkey} is not available (taken by another app); you can change it in Settings"
        },
        "toast.windowChanged" => if ru {
            "Окно сменилось: картинки остались в буфере, вставьте их сами"
        } else {
            "The window changed: the pictures are in the clipboard, paste them yourself"
        },
        "toast.copyFailed" => if ru { "Не удалось скопировать" } else { "Could not copy" },
        "toast.pinFailed" => if ru { "Не удалось закрепить снимок" } else { "Could not pin the screenshot" },
        "toast.pinsFull" => if ru {
            "Закреплено уже {max} снимков. Закройте ненужные и попробуйте снова"
        } else {
            "{max} screenshots are pinned already. Close some and try again"
        },
        "toast.sessionFull" => if ru { "В наборе уже 26 снимков" } else { "The set already has 26 screenshots" },
        "toast.screenPermissionOff" => if ru { "Нет доступа к записи экрана" } else { "Screen Recording is off" },
        "toast.screenPermissionHint" => if ru {
            "Включите WellSnap в настройках и перезапустите"
        } else {
            "Allow WellSnap in Settings, then reopen it"
        },
        "toast.copiedPicture" => if ru { "Скопировано одной картинкой" } else { "Copied as one image" },
        "toast.pictureCramped" => if ru {
            "Снимков много: мелкий текст может не читаться. Для такого набора лучше альбом"
        } else {
            "Many screenshots: small text may be hard to read. An album works better for this set"
        },
        "toast.pictureFailed" => if ru { "Не удалось собрать картинку" } else { "Could not build the image" },
        "tray.captureOff" => if ru {
            "Сделать скриншот ({hotkey} не работает)"
        } else {
            "Take screenshot ({hotkey} is not active)"
        },
        "tray.fixHotkey" => if ru { "Исправить {hotkey}..." } else { "Fix {hotkey}..." },
        "toast.hotkeyOff" => if ru { "{hotkey} не работает для WellSnap" } else { "{hotkey} does not work for WellSnap" },
        "toast.hotkeyOffBy" => if ru {
            "Клавишу заняла программа {app}. Как исправить: значок WellSnap в трее > «Настройки...» > «Горячие клавиши»."
        } else {
            "{app} is using it. To fix it: WellSnap tray icon > Settings... > Shortcuts."
        },
        "toast.hotkeyOffUnknown" => if ru {
            "Клавишу заняла другая программа. Как исправить: значок WellSnap в трее > «Настройки...» > «Горячие клавиши»."
        } else {
            "Another app is using it. To fix it: WellSnap tray icon > Settings... > Shortcuts."
        },
        "toast.noUpload" => if ru {
            "WellSnap ничего не загружает в интернет. Снимок в буфере: вставьте его в чат через {paste}."
        } else {
            "WellSnap does not upload anything. The screenshot is on the clipboard: paste it into the chat with {paste}."
        },
        "toast.pasteJoined" => if ru {
            "Сюда вставляется одна картинка, поэтому снимки собраны в одну"
        } else {
            "This app takes one image at a time, so the screenshots were joined into one"
        },
        "toast.pastedPicture" => if ru { "Вставлено одной картинкой" } else { "Pasted as one image" },
        "toast.pasteGrouped" => if ru {
            "{app} принимает до {max} картинок за раз, поэтому часть снимков собрана по несколько на картинку"
        } else {
            "{app} takes up to {max} images at a time, so some screenshots were joined into shared images"
        },
        "toast.pasteCancelled" => if ru { "Окно сменилось, вставка отменена" } else { "The window changed, paste cancelled" },
        "toast.pasteStopped" => if ru {
            "Окно сменилось, вставка остановлена: вставлено {done} из {total}"
        } else {
            "The window changed, paste stopped: {done} of {total} pasted"
        },
        "toast.pasteManual" => if ru { "В терминал WellSnap сам не вставляет" } else { "WellSnap does not paste into terminals" },
        "toast.pasteManualText" => if ru {
            "Текст комментариев в буфере, вставьте его, если нужно"
        } else {
            "The comment text is in the clipboard if you need it"
        },
        "toast.pasteTextOnly" => if ru {
            "Сюда вставляется только текст, поэтому вставлены одни комментарии"
        } else {
            "This app takes text only, so only the comments were pasted"
        },
        "toast.pasteNoText" => if ru {
            "Сюда вставляется только текст, а комментариев нет: вставлять нечего"
        } else {
            "This app takes text only, and there are no comments: nothing to paste"
        },
        "block.legend" => if ru { "Картинки по порядку:" } else { "Images in order:" },
        "block.snapshots" => if ru { "Снимки" } else { "Screenshots" },
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_mapping() {
        assert_eq!(Lang::from_locale(Some("ru-RU")), Lang::Ru);
        assert_eq!(Lang::from_locale(Some("ru")), Lang::Ru);
        assert_eq!(Lang::from_locale(Some("kk_KZ")), Lang::Ru);
        assert_eq!(Lang::from_locale(Some("en-US")), Lang::En);
        assert_eq!(Lang::from_locale(Some("de-DE")), Lang::En);
        assert_eq!(Lang::from_locale(None), Lang::En);
    }

    /// Every key of `tr`, read from this file, so a new string is checked without being listed here.
    fn all_keys() -> Vec<&'static str> {
        let source = include_str!("i18n.rs");
        let start = source.find("pub fn tr(").unwrap();
        // `lines` also drops the \r of a CRLF checkout.
        source[start..]
            .lines()
            .take_while(|line| *line != "}")
            .filter_map(|line| line.trim_start().strip_prefix('"')?.split_once("\" =>").map(|(key, _)| key))
            .collect()
    }

    #[test]
    fn every_key_is_translated_without_long_dashes() {
        let keys = all_keys();
        assert!(keys.len() > 40, "the scan found only {} keys", keys.len());
        assert!(keys.contains(&"toast.copiedPicture") && keys.contains(&"block.snapshots"));
        for key in keys {
            for lang in [Lang::Ru, Lang::En] {
                let text = tr(lang, key);
                assert!(!text.trim().is_empty(), "{key} is empty in {lang:?}");
                let long_dash = text.chars().any(|c| ('\u{2012}'..='\u{2015}').contains(&c));
                assert!(!long_dash, "{key} has a long dash in {lang:?}");
            }
        }
    }

    #[test]
    fn known_keys_are_translated() {
        for key in ["tray.capture", "tray.quit", "block.snapshot", "toast.copied", "toast.smartPasteOff", "toast.windowChanged"] {
            assert!(!tr(Lang::Ru, key).is_empty());
            assert!(!tr(Lang::En, key).is_empty());
        }
    }
}
