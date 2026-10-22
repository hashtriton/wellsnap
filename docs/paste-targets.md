# Where paste with text works

[Русская версия](paste-targets.ru.md)

## What paste with text is

"Copy all" in the snapshot strip puts every screenshot on the clipboard as files, together with the comment text. Most programs take only part of that from one Ctrl+V (⌘V on macOS): a chat shows the files and drops the text.

**Paste with text** (Win+Alt+V on Windows and Ctrl+⌘V on macOS by default, changed in Settings > Shortcuts) pastes in several steps instead, usually the comment text first and then the screenshots, so both arrive in one message. It never presses Enter: you check the message and send it yourself.

How it pastes depends on the program in front. This page lists the programs WellSnap recognises, what it does in each of them and whether that was checked by hand.

## How to read the status

- **Verified: date, versions.** We pasted 3, 10, 11 and 26 screenshots by hand into this version and recorded the result.
- **On, not verified yet.** Every WellSnap build uses this rule, but it has not been checked by hand for this release yet.
- **Not verified.** WellSnap pastes as into any unknown program: the text first, then all screenshots as files. The column "What paste with text does" shows the planned behaviour, which test builds already use.

The column "Plain Ctrl+V after Copy all" says what one ordinary paste gives. "Expected" means it comes from the program's source code or documentation and was not checked by hand.

## Privacy

WellSnap recognises the program by its file name (the bundle id on macOS) and, in browsers, by the window title, only on this computer and only at the moment of the paste. The title is not saved and not sent anywhere. The local log records only which row of this page was used, for example `gmail`; a crash report, if you allow them, can contain the last lines of that log. Anonymous usage statistics (Settings > Privacy) count pastes only by broad kind of program, such as messenger, mail or browser.

## Messengers

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| Telegram Desktop | Windows, macOS | The text becomes the album caption. Text over 1024 characters or more than 10 screenshots: the albums are pasted and the text stays in the clipboard for a separate message | Albums without the text | On, not verified yet | `tg_desktop` |
| AyuGram, Kotatogram, 64Gram | Windows | As in Telegram Desktop | Albums without the text (expected) | Not verified | `tg_fork` |
| Telegram Web K (web.telegram.org/k) | Browser | The text becomes the album caption; with more than 10 screenshots Telegram sends the text first. Text over 1024 characters stays in the clipboard | Albums without the text (expected) | Not verified | `tg_web_k` |
| Telegram Web A (web.telegram.org/a) | Browser | As in Telegram Web K | Albums without the text (expected) | Not verified | `tg_web_a` |
| WhatsApp | Windows, browser | The text, then all screenshots as attachments of one message | Screenshots without the text (expected) | Not verified | `whatsapp` |
| WhatsApp | macOS | The text, then all screenshots as attachments of one message | Screenshots without the text (expected) | Not verified | `whatsapp_mac` |
| MAX | Browser | The text, then all screenshots as attachments of one message | Screenshots without the text (expected) | Not verified | `max` |
| VK messages | Browser | The text, then all screenshots as attachments. More than 10: joined into 10 pictures, several screenshots each | Screenshots without the text (expected) | Not verified | `vk_messages` |
| Yandex Messenger | Browser | The text, then all screenshots as attachments of one message | Screenshots without the text (expected) | Not verified | `yandex_messenger` |
| Discord | Windows, macOS, browser | The text, then all screenshots as attachments. More than 10: joined into 10 pictures, several screenshots each | Screenshots without the text (expected) | Not verified | `discord` |
| Slack | Windows, macOS, browser | The text, then all screenshots as attachments. More than 10: joined into 10 pictures, several screenshots each | Screenshots without the text (expected) | Not verified | `slack` |
| Microsoft Teams | Windows, macOS, browser | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `teams` |
| Viber | Windows, macOS | The text, then all screenshots as attachments of one message | Not known yet | Not verified | `viber` |
| Zoom | Windows, macOS | The text, then all screenshots as attachments of one message | Not known yet | Not verified | `zoom` |

## Mail

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| Outlook (classic) | Windows | One paste of the whole Copy all: pictures in the text, the comments under each. Pictures over 16 MB in total: the text, then one joined picture | Screenshots as attachments (expected) | Not verified | `outlook_classic` |
| Outlook (new) | Windows | The text, then all screenshots as attachments of one message | Pictures in the text (expected) | Not verified | `outlook_new` |
| Outlook | macOS | The text, then all screenshots as files | Not known yet | Not verified | `outlook_mac` |
| Outlook on the web | Browser | The text, then all screenshots as attachments of one message | Pictures in the text (expected) | Not verified | `outlook_web` |
| Thunderbird, Betterbird | Windows, macOS | One paste of the whole Copy all: pictures in the text, the comments under each. Pictures over 16 MB in total: the text, then one joined picture | Pictures in the text, the comments under each (expected) | Not verified | `thunderbird` |
| eM Client | Windows, macOS | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Pictures in the text or as attachments (expected) | Not verified | `emclient` |
| The Bat! | Windows | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `thebat` |
| Apple Mail | macOS | The text, then all screenshots as attachments of one message | Screenshots as attachments (expected) | Not verified | `apple_mail` |
| Gmail | Browser | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `gmail` |
| Yandex Mail | Browser | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `yandex_mail` |
| Mail.ru | Browser | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `mailru` |

## Social networks

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| VK (feed, posts, comments) | Browser | The text, then all screenshots as attachments. More than 10: joined into 10 pictures, several screenshots each | Screenshots without the text (expected) | Not verified | `vk_social` |
| Odnoklassniki | Browser | The text, then all screenshots as attachments of one message | Screenshots without the text (expected) | Not verified | `ok` |
| Facebook | Browser | The text, then all screenshots as attachments of one message | Screenshots without the text (expected) | Not verified | `facebook` |
| X | Browser | The text, then all screenshots as attachments. More than 4: joined into 4 pictures, several screenshots each | Not known yet | Not verified | `x` |
| LinkedIn | Browser | The text, then all screenshots as attachments. More than 20: joined into 20 pictures, several screenshots each | Screenshots without the text (expected) | Not verified | `linkedin` |
| Instagram | Browser | The text, then all screenshots as files, as in an unknown program: the web composer is not known to take pasted pictures | Not known yet | Not verified | `instagram` |
| Threads | Browser | The text, then all screenshots as attachments. More than 20: joined into 20 pictures, several screenshots each | Screenshots without the text (expected) | Not verified | `threads` |
| Reddit | Browser | The text, then all screenshots as attachments. More than 20: joined into 20 pictures, several screenshots each | Not known yet | Not verified | `reddit` |
| Pikabu | Browser | The text, then all screenshots joined into one picture | Not known yet | Not verified | `pikabu` |
| Habr | Browser | The text, then all screenshots joined into one picture | Not known yet | Not verified | `habr` |

## AI chats

AI chats refer to pictures by their position, so a line in front of the text says which picture shows which screenshot, for example "Images in order: 1) Screenshot A; 2) Screenshots B, C".

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| ChatGPT | Windows, macOS, browser (start page only) | The order line and the text, then all screenshots as attachments. More than 10: joined into 10 pictures | Screenshots without the text (expected) | Not verified | `chatgpt` |
| Claude | Windows, macOS, browser | The order line and the text, then all screenshots as attachments. More than 20: joined into 20 pictures | Screenshots without the text (expected) | Not verified | `claude` |
| Gemini | Browser | The order line and the text, then all screenshots as attachments. More than 10: joined into 10 pictures | Screenshots without the text (expected) | Not verified | `gemini` |
| DeepSeek | Browser | The order line and the text, then all screenshots as attachments | Screenshots without the text (expected) | Not verified | `deepseek` |
| Alice (Yandex) | Browser | The text, then all screenshots joined into one picture | Not known yet | Not verified | `alice` |
| GigaChat | Browser | The text, then all screenshots joined into one picture | Not known yet | Not verified | `gigachat` |

## Work and documents

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| VS Code (chat) | Windows, macOS | The text, then all screenshots joined into one picture | Only the first screenshot (expected) | Not verified | `vscode` |
| Cursor | Windows, macOS | The text, then all screenshots joined into one picture | Not known yet | Not verified | `cursor` |
| Jira | Browser | The text, then all screenshots joined into one picture | Only one screenshot is kept when creating an issue (expected) | Not verified | `jira` |
| Confluence | Browser | The text, then all screenshots joined into one picture | Not known yet | Not verified | `confluence` |
| Notion | Windows, macOS | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `notion` |
| Google Docs | Browser | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `gdocs` |
| Word | Windows, macOS | The text, then each screenshot pasted as a separate picture. More than 10: joined into 10 pictures | Not known yet | Not verified | `word` |

## Browsers

Pages that no row above recognises follow the rule of the browser's engine.

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| Firefox, LibreWolf, Waterfox, Floorp, Zen | Windows, macOS | The text, then all screenshots joined into one picture. A page from the tables above that takes several files gets one joined picture too | Only screenshot A (expected) | Not verified | `gecko_engine` |
| Any other program or page | Windows, macOS | The text, then all screenshots as files | Depends on the program | On, not verified yet | `unknown` |

## Terminals and editors

| Program | Windows / macOS | What paste with text does | Plain Ctrl+V after Copy all | Status | id |
|---|---|---|---|---|---|
| Notepad, Notepad++, TextEdit | Windows, macOS | Only the comment text | The comment text | Not verified | `text_editor` |
| Windows Terminal, Command Prompt, PowerShell, WezTerm, Alacritty, mintty, Terminal, iTerm2, Warp, Ghostty | Windows, macOS | Nothing is pasted: the comment text goes to the clipboard and WellSnap shows a notice. A shell may run a pasted text line by line | The comment text (expected) | On, not verified yet | `terminal` |

## Known limits

- Paste with text never presses Enter and never sends a message.
- Chromium-based programs (Chrome, Edge, Yandex Browser, Discord, Slack, WhatsApp, Claude, ChatGPT and others) hide the text while files are on the clipboard. That is why the text and the screenshots go in two pastes.
- Firefox and Thunderbird read only the first file of a pasted file list; Firefox before version 116 cannot paste files at all.
- Telegram compresses photos to 1280 pixels on the long side (2560 with large photos turned on). For full size, send the screenshots as files.
- X takes at most 4 pictures per post.
- Web pages are recognised by the window title. ChatGPT names its window after the conversation, so only the start page is recognised. A title rewritten by an extension, or a page title in an unusual form, gives the unknown-program behaviour.
- When the window changes during the paste, WellSnap stops pasting and shows a notice.

## A row is wrong or a program is missing

Open an issue with the [paste target form](https://github.com/hashtriton/wellsnap/issues/new?template=paste-target.yml): the program and its version, your system, the WellSnap version, how many screenshots you pasted, what you expected and what happened. A row that turns out broken goes back to "Not verified" in the next update.
