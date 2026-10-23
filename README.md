# WellSnap

Screenshots with arrows, blur and callouts, for Windows and macOS.

Press the hotkey, select an area, mark what matters and paste it into a chat. Several screenshots can be collected into a set (A, B, C...) with numbered comments and pasted into Telegram, WhatsApp, Discord, mail or a document in one go.

**По-русски:** WellSnap делает скриншоты со стрелками, размытием и нумерованными сносками. Несколько снимков с комментариями вставляются в чат одним сообщением. Программа бесплатная, без рекламы, с открытым исходным кодом.

## Features

- Area selection on a frozen screen, on every monitor.
- Arrow, pencil, rectangle, marker, blur and numbered callouts with comments.
- Copy, save to a folder, or add to a set; paste a whole set with text in one step ("paste with text").
- Paste that knows the target program: an album with a caption in Telegram, attachments in chats, pictures inline in Word and mail. See the [paste targets](docs/paste-targets.md).
- Ctrl+V (Cmd+V) pastes the screenshots together with the comment text while WellSnap's copy is on the clipboard; it can be turned off.
- Customizable hotkeys, Russian and English interface, light and dark themes.
- Quiet automatic updates.

## Download

Installers for Windows (10 and 11) and macOS (14 or later) are on the [Releases](https://github.com/hashtriton/wellsnap/releases) page. The Windows installer installs for the current user and does not need administrator rights.

## Privacy

WellSnap has no accounts and uploads no screenshots. It can send crash reports (Sentry) and anonymous daily usage statistics (PostHog); both can be turned off in the installer and under Settings > Privacy, and nothing is sent before the first setup is finished. Details: [privacy policy](PRIVACY.md) ([по-русски](PRIVACY.ru.md)).

## Code signing

The installers are not code signed yet. Releases are built from this repository by GitHub Actions ([release workflow](.github/workflows/release.yml)).

- Windows: SmartScreen shows "Windows protected your PC" on the first run. Click "More info", then "Run anyway".
- macOS: the app is not notarized yet, so macOS blocks the first launch. Open System Settings > Privacy & Security and click "Open Anyway".

## Build from source

Requirements: Node.js 24, Rust (stable) and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your system.

```sh
npm ci
npm run tauri dev      # run in development
npm run tauri build    # build the installer
```

A build from source sends no crash reports or statistics: the keys are added only by the release workflow. Rust tests: `cargo test` in `src-tauri`; frontend tests: `npm test`.

## License

[GNU General Public License v3.0 or later](LICENSE).
