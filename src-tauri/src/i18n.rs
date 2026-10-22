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
        "toast.sessionFull" => if ru { "В наборе уже 26 снимков" } else { "The set already has 26 screenshots" },
        "toast.screenPermissionOff" => if ru { "Нет доступа к записи экрана" } else { "Screen Recording is off" },
        "toast.screenPermissionHint" => if ru {
            "Включите WellSnap в настройках и перезапустите"
        } else {
            "Allow WellSnap in Settings, then reopen it"
        },
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

    #[test]
    fn known_keys_are_translated() {
        for key in ["tray.capture", "tray.quit", "block.snapshot", "toast.copied", "toast.smartPasteOff", "toast.windowChanged"] {
            assert!(!tr(Lang::Ru, key).is_empty());
            assert!(!tr(Lang::En, key).is_empty());
        }
    }
}
