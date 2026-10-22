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
}

export interface Snapshot {
  letter: string;
  width: number;
  height: number;
  marks: Mark[];
  revision: number;
}

export interface SetInfo {
  sessionId: string | null;
  snapshots: Snapshot[];
  nextLetter: string | null;
  text: string;
  smartPasteHotkey: string;
  smartPasteRegistered: boolean;
}

export interface HotkeyStatus {
  captureRegistered: boolean;
  captureError: string | null;
  smartPasteRegistered: boolean;
  smartPasteError: string | null;
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
  | { kind: "updated"; version: string; hasNotes: boolean; setCleared: boolean };

export const api = {
  overlayReady: (captureId: number) => invoke<void>("overlay_ready", { captureId }),
  overlayCancel: () => invoke<void>("overlay_cancel"),
  /** `image` is the encoded PNG/JPEG; metadata travels in a header to keep the body raw. */
  overlayFinish: (image: Uint8Array, meta: FinishMeta) =>
    invoke<void>("overlay_finish", image, { headers: { "x-meta": encodeURIComponent(JSON.stringify(meta)) } }),
  logMetric: (name: string, value: number) => invoke<void>("log_metric", { name, value }),

  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (patch: Partial<Omit<Settings, "donations">>) => invoke<Settings>("update_settings", { patch }),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  hotkeysSuspend: () => invoke<void>("hotkeys_suspend"),
  hotkeysResume: () => invoke<void>("hotkeys_resume"),
  validateHotkey: (accelerator: string) => invoke<void>("validate_hotkey", { accelerator }),

  getSet: () => invoke<SetInfo>("get_set"),
  setCopyAll: () => invoke<void>("set_copy_all"),
  setSaveAll: () => invoke<number>("set_save_all"),
  setClear: () => invoke<void>("set_clear"),
  setRemove: (letter: string) => invoke<void>("set_remove", { letter }),
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
};

/** URL of a snapshot image in the current set (served by the `wsnap` protocol). */
export function snapshotUrl(sessionId: string, letter: string, revision: number): string {
  return convertFileSrc(`snap/${sessionId}/${letter}/${revision}`, "wsnap");
}
