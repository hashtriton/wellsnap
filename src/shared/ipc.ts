// Typed bridge to the Rust core. Keep in sync with src-tauri/src/commands.rs and the payload structs.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Lang = "ru" | "en";
export type LanguagePref = "auto" | "ru" | "en";
export type Theme = "system" | "light" | "dark";
export type ImageFormat = "png" | "jpg";
export type EnterAction = "copy" | "save";
export type CommentPlacement = "onImage" | "separate";
export type DonationStatus = "active" | "supporter" | "never";
export type PromptOutcome = "support" | "later" | "alreadySupported" | "never" | "notSeen";
export type Platform = "windows" | "macos" | "other";
/** How the strip's "Copy all" and "Save all" hand over a set: files (an album in chats) or one composed picture. */
export type SetCopyMode = "album" | "onePicture";
/** Why a hotkey is not registered: another program holds it, WellSnap refuses it, or anything else. */
export type HotkeyErrorKind = "taken" | "reserved" | "other";
/** Extra meaning of a finish: "noUpload" is Lightshot's upload key (Ctrl+D), the toast explains there is no upload. */
export type FinishHint = "noUpload";
export type ContenderId =
  | "oneDrive"
  | "dropbox"
  | "lightshot"
  | "yandexDisk"
  | "shareX"
  | "greenshot"
  | "flameshot"
  | "snagit"
  | "picPick"
  | "gyazo"
  | "screenpresso"
  | "fastStone"
  | "joxi";
/** Crash reports: sent without asking, sent after a prompt, or not captured at all. */
export type CrashMode = "always" | "ask" | "never";
/** Crash card answers ("send", "dontSend", "always"); Settings > Privacy uses "send" and "delete". */
export type CrashAnswer = "send" | "dontSend" | "always" | "delete";
export type JsErrorKind = "error" | "unhandledRejection";
export type SentKind = "crash" | "error" | "usage";

export interface DonationState {
  firstRunAt: number;
  captures: number;
  marks: number;
  activeDays: number;
  lastActiveDay: string;
  promptsShown: number;
  lastPromptAt: number;
  nextEligibleAt: number;
  status: DonationStatus;
}

export interface Settings {
  language: LanguagePref;
  theme: Theme;
  captureHotkey: string;
  smartPasteHotkey: string;
  /** Ctrl+V (Cmd+V) does paste with text while WellSnap's own copy is on the clipboard. */
  ctrlVPaste: boolean;
  autostart: boolean;
  saveDir: string | null;
  imageFormat: ImageFormat;
  jpgQuality: number;
  /** Where mark comments go: drawn on the screenshot, or kept as separate text (the image keeps only numbers). */
  commentPlacement: CommentPlacement;
  enterAction: EnterAction;
  showHints: boolean;
  hideStripFromCapture: boolean;
  /** Owned by the overlay editor: per-tool color and width. */
  toolPrefs: Record<string, unknown>;
  onboardingDone: boolean;
  /** Background update check on GitHub shortly after start and every 12 hours. The manual check always works. */
  autoCheckUpdates: boolean;
  /** A found update installs itself while the computer is idle; needs `autoCheckUpdates`. */
  autoInstallUpdates: boolean;
  /** The user once gave Print Screen to WellSnap (Windows); kept so a reinstall can restore the take-over. */
  printScreenTakenOver: boolean;
  /** Remembered toggle on the strip. */
  setCopyMode: SetCopyMode;
  /** Core-owned: fingerprint of the last startup warning about a taken capture hotkey. */
  hotkeyWarning: string | null;
  /** Only builds with telemetry send anything (`AppInfo.telemetry`), and only after onboarding. */
  crashReports: CrashMode;
  /** Anonymous daily usage totals; same conditions as `crashReports`. */
  usageStats: boolean;
  donations: DonationState;
}

export interface MonitorInfo {
  id: string;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
}

export interface PreparePayload {
  captureId: number;
  label: string;
  monitor: MonitorInfo;
  /** "shared": pixels came with the payload (WebView2 shared memory); "protocol": fetch `framePath`. */
  transport: "shared" | "protocol";
  framePath: string;
  width: number;
  height: number;
  /** Letter of the next snapshot when a set already exists, else null. */
  setLetter: string | null;
  setCount: number;
  setFull: boolean;
  captureMs: number;
}

export interface Mark {
  num: number;
  comment: string;
}

export type FinishAction = "copy" | "save" | "addToSet" | "copyAll";

export interface FinishMeta {
  captureId: number;
  action: FinishAction;
  width: number;
  height: number;
  letter: string | null;
  marks: Mark[];
  /** True when the comments are drawn on the image, so they must not be repeated as clipboard text. */
  commentsOnImage: boolean;
  format: ImageFormat;
  /** Physical px per CSS px of the capture monitor, so snapshots from mixed-DPI monitors compose at one size. */
  scale: number;
  hint?: FinishHint;
  /** Annotations per drawing tool (keys from DRAW_TOOLS), for the anonymous usage counters. */
  tools: Record<string, number>;
}

export interface Snapshot {
  letter: string;
  width: number;
  height: number;
  marks: Mark[];
  /** The comments are drawn on the image, so they are not repeated as text. */
  commentsOnImage: boolean;
  revision: number;
  scale: number;
}

export interface SetInfo {
  sessionId: string | null;
  snapshots: Snapshot[];
  nextLetter: string | null;
  text: string;
  smartPasteHotkey: string;
  smartPasteRegistered: boolean;
  /** Ctrl+V (Cmd+V) is watched: after "Copy all" it pastes with the text. */
  ctrlVPaste: boolean;
  copyMode: SetCopyMode;
}

export interface HotkeyStatus {
  captureRegistered: boolean;
  captureError: string | null;
  captureErrorKind: HotkeyErrorKind | null;
  smartPasteRegistered: boolean;
  smartPasteError: string | null;
  smartPasteErrorKind: HotkeyErrorKind | null;
}

export interface ContenderRef {
  id: ContenderId;
  /** Display name in the UI language. */
  name: string;
}

/** Known screenshot tools that may hold the capture key (Windows; empty on macOS). */
export interface ConflictReport {
  running: ContenderRef[];
  /** Most likely owners of a taken capture key, most likely first. */
  suspects: ContenderRef[];
  /** May react to the same key although WellSnap registered it. */
  mayDouble: ContenderRef[];
  /** Free keys to offer instead, in accelerator syntax. */
  alternatives: string[];
}

export interface PrintScreenStatus {
  applicable: boolean;
  snippingEnabled: boolean;
  onedriveEnabled: boolean;
  changedByUs: boolean;
}

export interface AppInfo {
  version: string;
  platform: Platform;
  lang: Lang;
  hotkeys: HotkeyStatus;
  printScreen: PrintScreenStatus;
  screenPermission: boolean;
  inputPermission: boolean;
  saveDir: string;
  /** This build can send crash reports and statistics; otherwise it sends nothing at all. */
  telemetry: boolean;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

/** A version installed in the last 14 days, for "What's new" in About. `at` is Unix seconds. */
export interface RecentUpdate {
  from: string | null;
  version: string;
  notes: string | null;
  at: number;
}

export interface UpdateStatus {
  phase: "idle" | "available" | "installing";
  info: UpdateInfo | null;
  /** The available version may install by itself once the settings allow it. */
  autoEligible: boolean;
  recent: RecentUpdate | null;
}

export type NoticePayload =
  | { kind: "toast"; text: string; detail: string | null; error: boolean }
  | { kind: "nudge"; index: number; captures: number }
  | { kind: "updated"; version: string; hasNotes: boolean; setCleared: boolean }
  /** After a crash in "ask" mode; `count` reports are waiting. */
  | { kind: "crashPrompt"; count: number };

/** One-picture sheet the strip composes; `title` is the heading word ("Снимок" / "Screenshot"). */
export interface SheetRequest {
  requestId: number;
  sessionId: string;
  title: string;
  /** Draw the comment lines under each snapshot. */
  withComments: boolean;
  snapshots: Snapshot[];
}

export interface SheetPutMeta {
  requestId: number;
  width: number;
  height: number;
  /** Too many snapshots to keep the text readable. */
  cramped: boolean;
}

export interface JsErrorReport {
  kind: JsErrorKind;
  message: string;
  stack: string | null;
  /** `globalThis._sentryDebugIds`: script URL to debug id. */
  debugIds: Record<string, string>;
}

export interface TelemetryStatus {
  /** Crash reports waiting to be sent. */
  pendingReports: number;
}

/** One line of "Show what was sent". `at` is Unix seconds. */
export interface SentItem {
  at: number;
  kind: SentKind;
  summary: string;
  detail: string | null;
}

export const api = {
  overlayReady: (captureId: number) => invoke<void>("overlay_ready", { captureId }),
  overlayCancel: () => invoke<void>("overlay_cancel"),
  /** `image` is the encoded PNG/JPEG; metadata travels in a header to keep the body raw. */
  overlayFinish: (image: Uint8Array, meta: FinishMeta) =>
    invoke<void>("overlay_finish", image, { headers: { "x-meta": encodeURIComponent(JSON.stringify(meta)) } }),
  logMetric: (name: string, value: number) => invoke<void>("log_metric", { name, value }),

  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Omit<Settings, "donations" | "printScreenTakenOver" | "hotkeyWarning">>) =>
    invoke<Settings>("update_settings", { patch }),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  hotkeysSuspend: () => invoke<void>("hotkeys_suspend"),
  hotkeysResume: () => invoke<void>("hotkeys_resume"),
  validateHotkey: (accelerator: string) => invoke<void>("validate_hotkey", { accelerator }),
  /** Scans the running programs for known screenshot tools (Windows). */
  hotkeyConflicts: () => invoke<ConflictReport>("hotkey_conflicts"),
  /** Registers the hotkeys again and returns the outcome. */
  hotkeysRecheck: () => invoke<HotkeyStatus>("hotkeys_recheck"),

  getSet: () => invoke<SetInfo>("get_set"),
  setCopyAll: () => invoke<void>("set_copy_all"),
  setSaveAll: () => invoke<number>("set_save_all"),
  setClear: () => invoke<void>("set_clear"),
  setRemove: (letter: string) => invoke<void>("set_remove", { letter }),
  /** `png` is the composed sheet; metadata travels in a header to keep the body raw. */
  sheetPut: (png: Uint8Array, meta: SheetPutMeta) =>
    invoke<void>("sheet_put", png, { headers: { "x-meta": encodeURIComponent(JSON.stringify(meta)) } }),
  sheetFailed: (requestId: number, reason: string) => invoke<void>("sheet_failed", { requestId, reason }),
  /** Called once the strip listens; returns the request that may have been sent before that. */
  sheetPending: () => invoke<SheetRequest | null>("sheet_pending"),
  startCapture: () => invoke<void>("start_capture"),

  openSettings: (tab?: string) => invoke<void>("open_settings", { tab: tab ?? null }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),

  printScreenStatus: () => invoke<PrintScreenStatus>("print_screen_status"),
  printScreenTakeOver: () => invoke<PrintScreenStatus>("print_screen_take_over"),
  printScreenRestore: () => invoke<PrintScreenStatus>("print_screen_restore"),

  donationOutcome: (outcome: PromptOutcome) => invoke<void>("donation_outcome", { outcome }),
  donationSetStatus: (supporter: boolean, reminders: boolean) =>
    invoke<void>("donation_set_status", { supporter, reminders }),
  donationResetCounters: () => invoke<void>("donation_reset_counters"),

  noticeClose: () => invoke<void>("notice_close"),
  /** Called once the notice page listens; returns the notice that may have been emitted before that. */
  noticeReady: () => invoke<NoticePayload | null>("notice_ready"),
  onboardingComplete: () => invoke<void>("onboarding_complete"),
  requestScreenPermission: () => invoke<boolean>("request_screen_permission"),
  requestInputPermission: () => invoke<boolean>("request_input_permission"),
  /** "keyboard" (Windows Print Screen toggle), "screen" / "accessibility" (macOS privacy panes). */
  openSystemSettings: (kind: "keyboard" | "screen" | "accessibility") => invoke<void>("open_system_settings", { kind }),
  relaunch: () => invoke<void>("relaunch"),

  /** Known update state and the version installed recently, without a network request. */
  getUpdate: () => invoke<UpdateStatus>("get_update"),
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  /** Downloads, verifies and installs, then restarts the app (does not resolve on success). */
  installUpdate: () => invoke<void>("install_update"),
  hideUpdateNotes: () => invoke<void>("hide_update_notes"),

  reportJsError: (report: JsErrorReport) => invoke<void>("report_js_error", { report }),
  telemetryStatus: () => invoke<TelemetryStatus>("telemetry_status"),
  /** The most recent uploads, newest first. */
  telemetryHistory: () => invoke<SentItem[]>("telemetry_history"),
  crashPromptAnswer: (answer: CrashAnswer) => invoke<void>("crash_prompt_answer", { answer }),
  /** "Reset install number": new random ids. */
  telemetryResetIds: () => invoke<void>("telemetry_reset_ids"),
};

export const events = {
  onPrepare: (cb: (p: PreparePayload) => void): Promise<UnlistenFn> =>
    listen<PreparePayload>("overlay:prepare", (e) => cb(e.payload)),
  onOverlayReset: (cb: () => void) => listen("overlay:reset", () => cb()),
  onSettingsChanged: (cb: (s: Settings) => void) => listen<Settings>("settings:changed", (e) => cb(e.payload)),
  onSessionChanged: (cb: () => void) => listen("session:changed", () => cb()),
  onHotkeysStatus: (cb: (s: HotkeyStatus) => void) => listen<HotkeyStatus>("hotkeys:status", (e) => cb(e.payload)),
  onNotice: (cb: (p: NoticePayload) => void) => listen<NoticePayload>("notice:show", (e) => cb(e.payload)),
  onNoticeHide: (cb: () => void) => listen("notice:hide", () => cb()),
  onSettingsTab: (cb: (tab: string) => void) => listen<string>("settings:tab", (e) => cb(e.payload)),
  onUpdateStatus: (cb: (s: UpdateStatus) => void) => listen<UpdateStatus>("update:status", (e) => cb(e.payload)),
  onSheetRequest: (cb: (r: SheetRequest) => void) => listen<SheetRequest>("sheet:request", (e) => cb(e.payload)),
};

/** URL of a snapshot image in the current set (served by the `wsnap` protocol). */
export function snapshotUrl(sessionId: string, letter: string, revision: number): string {
  return convertFileSrc(`snap/${sessionId}/${letter}/${revision}`, "wsnap");
}
