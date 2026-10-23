# WellSnap privacy policy

[По-русски](PRIVACY.ru.md)

WellSnap is a screenshot app that works on your computer. It has no accounts and uploads nothing: screenshots, comments, the clipboard, file names and window titles stay on your computer.

The app sends data to other systems only in the cases below. Crash reports and statistics can be turned off.

## Crash reports (Sentry)

Purpose: find and fix bugs.

When WellSnap crashes or hits an error, it can send a report to [Sentry](https://sentry.io/) (data region: Germany, EU). A report contains:

- the app, system and WebView2 versions, the processor architecture and the UI language;
- where in the code the failure happened (a stack trace) and the last lines of the app's technical log;
- after a sudden crash, a small snapshot of the program's memory (a minidump). It can hold fragments of what WellSnap was working on at that moment. Sentry processes it and does not keep the file;
- a random install number, not linked to your name or account.

Before a report leaves your computer, WellSnap removes your user name, computer name, home folder, the custom save folder and screenshot file names from it. Screenshots, comments and files are never attached. Sentry is set not to store IP addresses.

Settings > Privacy > Crash reports: "Always send", "Ask each time" (only after a crash, WellSnap asks first) or "Never" (nothing is recorded or sent).

## Anonymous usage statistics (PostHog)

Purpose: understand which features are used and how fast the app works.

Once a day, WellSnap can send totals for the day to [PostHog](https://posthog.com/) (data region: USA):

- how many screenshots were taken, cancelled or failed, which tools and buttons were used, how many marks and comments were added;
- how pastes ended, counted only by the kind of program (messenger, mail, browser, office, other), never by its name or window title;
- how fast the editor opened, the number and scale of monitors;
- some settings: theme, image format, whether a custom save folder is set (not the folder itself), autostart, update options, hotkeys changed or not;
- the app, system and WebView2 versions, the processor architecture, the UI language, the week of installation;
- the country, which PostHog derives from the IP address. The IP address itself, the city and the coordinates are not stored;
- a random install number, not linked to your name or account.

Settings > Privacy > "Send anonymous usage statistics". Turning it off deletes the install number and the totals not sent yet.

## When it starts and how to turn it off

- Nothing is sent before the first setup window is finished.
- The Windows installer shows this information and lets you turn off crash reports and statistics before installing.
- Any time later: Settings > Privacy. "Show what was sent" lists what left your computer; "Reset install number" starts a new random number.
- Builds compiled from source without the telemetry keys send nothing.

## The Ctrl+V key

While WellSnap's own copy with comment text is on the clipboard, WellSnap watches one key combination, Ctrl+V (Cmd+V on macOS), to paste the screenshots together with the text. It does not look at other keys, and it records and sends no key presses. Turn it off under Settings > Shortcuts.

## Other network requests

- **Updates.** WellSnap asks GitHub for the latest version shortly after start and every 12 hours, and downloads new versions from GitHub. Nothing else is sent with this check. Turn it off under Settings > About.
- **Links.** Buttons such as "Support the project" or "Report a bug" open a page in your browser. WellSnap itself sends nothing there.

## Third-party services

- Sentry: [privacy policy](https://sentry.io/privacy/)
- PostHog: [privacy policy](https://posthog.com/privacy)
- GitHub (updates): [privacy statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement)

The data is used only to fix bugs and improve WellSnap. It is not sold and not used for advertising.

## Questions and deletion

The data carries no name, account or contact, so it cannot be matched to a person. "Reset install number" in Settings > Privacy separates everything sent later from what was sent before. Sentry and PostHog delete old data under their own retention rules.

Questions: <https://github.com/hashtriton/wellsnap/issues>.

Changes to this policy are published in this repository.
