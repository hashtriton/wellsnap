// What to tell the user about the capture hotkey, from the core's registration status, the Windows Print Screen
// setting and the scan for known screenshot tools. Shared by Settings > Shortcuts and onboarding.
import type { TFunction } from "../i18n";
import type { ConflictReport, ContenderId, ContenderRef, HotkeyStatus, Platform, PrintScreenStatus } from "../ipc";
import { sameAccelerator } from "./hotkey";

export type CaptureAdvice =
  | { kind: "ok" }
  /** Windows 11 gives Print Screen to Snipping Tool: PrintScreenCard explains it, as before. */
  | { kind: "snipping" }
  | { kind: "takenBy"; app: ContenderRef }
  | { kind: "takenMany"; apps: ContenderRef[] }
  | { kind: "takenUnknown" }
  | { kind: "reserved" }
  /** Registered, but a running tool may react to the same key too. */
  | { kind: "mayDouble"; apps: ContenderRef[] }
  | { kind: "other" };

export type TakenAdvice = Extract<CaptureAdvice, { kind: "takenBy" | "takenMany" | "takenUnknown" }>;

export interface AdviceInput {
  hotkey: string;
  status: HotkeyStatus | null | undefined;
  printScreen: PrintScreenStatus | null;
  report: ConflictReport | null;
  platform: Platform;
}

/**
 * Null while nothing is known or the hotkeys are suspended for the recorder. The conflict scan is Windows only:
 * macOS reports every failure the same way, so it gets "other" and its usual texts.
 */
export function captureAdvice({ hotkey, status, printScreen, report, platform }: AdviceInput): CaptureAdvice | null {
  const windows = platform === "windows";
  if (windows && printScreen?.applicable && printScreen.snippingEnabled && sameAccelerator(hotkey, "PrintScreen", platform)) {
    return { kind: "snipping" };
  }
  if (!status) return null;
  if (status.captureRegistered) {
    const apps = windows ? (report?.mayDouble ?? []) : [];
    return apps.length > 0 ? { kind: "mayDouble", apps } : { kind: "ok" };
  }
  const kind = status.captureErrorKind ?? (status.captureError ? "other" : null);
  if (kind === null) return null;
  if (kind === "reserved") return { kind: "reserved" };
  if (kind === "other" || !windows) return { kind: "other" };
  const suspects = report?.suspects ?? [];
  if (suspects.length === 1) return { kind: "takenBy", app: suspects[0] };
  if (suspects.length > 1) return { kind: "takenMany", apps: suspects };
  return { kind: "takenUnknown" };
}

export function isTaken(advice: CaptureAdvice | null): advice is TakenAdvice {
  return advice?.kind === "takenBy" || advice?.kind === "takenMany" || advice?.kind === "takenUnknown";
}

/** "A", "A или B", "A, B или C". */
export function joinNames(names: string[], t: TFunction): string {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} ${t("settings.conflicts.or")} ${names[names.length - 1]}`;
}

/** Status line that names the owner of a taken key; null keeps the generic "taken" text. */
export function takenText(advice: CaptureAdvice | null, hotkey: string, t: TFunction): string | null {
  if (advice?.kind === "takenBy") return t("settings.hotkeys.statusTakenBy", { app: advice.app.name, hotkey });
  if (advice?.kind === "takenMany") {
    return t("settings.hotkeys.statusTakenByMany", { hotkey, apps: joinNames(advice.apps.map((a) => a.name), t) });
  }
  return null;
}

/**
 * Onboarding "Give it a try": why pressing the capture key would not reach WellSnap right now, or null when
 * it would. Windows 11 Snipping Tool and a program that took the key both send the user to the button.
 */
export function blockedText(advice: CaptureAdvice | null, hotkey: string, t: TFunction): string | null {
  switch (advice?.kind) {
    case "snipping":
      return t("onboarding.tryIt.snipping", { hotkey });
    case "takenBy":
      return t("onboarding.tryIt.takenBy", { hotkey, app: advice.app.name });
    case "takenMany":
      return t("onboarding.tryIt.takenBy", { hotkey, app: joinNames(advice.apps.map((a) => a.name), t) });
    case "takenUnknown":
      return t("onboarding.tryIt.takenUnknown", { hotkey });
    default:
      return null;
  }
}

/** Translation key of the steps that free the key in that program; OneDrive keeps its Print Screen text. */
export function stepsKey(id: ContenderId): string {
  return id === "oneDrive" ? "settings.printScreen.onedrive" : `settings.conflicts.steps.${id}`;
}

export function suspectsOf(advice: CaptureAdvice | null): ContenderRef[] {
  if (advice?.kind === "takenBy") return [advice.app];
  if (advice?.kind === "takenMany") return advice.apps;
  return [];
}

/** PrintScreenCard already shows the OneDrive notice; the conflict card does not repeat it. */
export function oneDriveShown(printScreen: PrintScreenStatus | null): boolean {
  return !!printScreen?.applicable && printScreen.onedriveEnabled;
}

/** Steps for the suspected owners, without the OneDrive text while PrintScreenCard shows it. */
export function fixSteps(apps: ContenderRef[], printScreen: PrintScreenStatus | null, t: TFunction): string[] {
  return apps.filter((a) => a.id !== "oneDrive" || !oneDriveShown(printScreen)).map((a) => t(stepsKey(a.id)));
}

/** Notes about tools that may react to the same key, minus the hidden ones and the OneDrive one PrintScreenCard shows. */
export function doubleNotes(advice: CaptureAdvice | null, printScreen: PrintScreenStatus | null, hidden: readonly string[]): ContenderRef[] {
  if (advice?.kind !== "mayDouble") return [];
  return advice.apps.filter((a) => !hidden.includes(a.id) && (a.id !== "oneDrive" || !oneDriveShown(printScreen)));
}

/** The free key the core suggests instead of a taken one, unless it is the current key. */
export function alternativeKey(advice: CaptureAdvice | null, report: ConflictReport | null, hotkey: string, platform: Platform): string | null {
  if (!isTaken(advice)) return null;
  return report?.alternatives.find((a) => !sameAccelerator(a, hotkey, platform)) ?? null;
}
