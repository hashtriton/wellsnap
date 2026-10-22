// Status line under a hotkey in Settings, derived from the core's HotkeyStatus.
import type { HotkeyErrorKind, HotkeyStatus } from "../shared/ipc";
import { errorKind } from "../shared/ui/conflicts";

export type RowStatus = "ok" | "idle" | "taken" | "reserved" | null;

/** "reserved": WellSnap refuses a copy, paste or undo chord; every other failure reads as "taken". */
function errorStatus(kind: HotkeyErrorKind | null, error: string | null): RowStatus {
  const k = errorKind(kind, error);
  if (k === "reserved") return "reserved";
  return k ? "taken" : null;
}

export function captureStatus(s: HotkeyStatus | undefined): RowStatus {
  if (!s) return null;
  if (s.captureRegistered) return "ok";
  return errorStatus(s.captureErrorKind, s.captureError);
}

/** Smart paste is registered only while there is something to paste, so "not registered" is normal. */
export function smartPasteStatus(s: HotkeyStatus | undefined): RowStatus {
  if (!s) return null;
  if (s.smartPasteRegistered) return "ok";
  return errorStatus(s.smartPasteErrorKind, s.smartPasteError) ?? "idle";
}
