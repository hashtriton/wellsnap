// Status line under a hotkey in Settings, derived from the core's HotkeyStatus.
import type { HotkeyStatus } from "../shared/ipc";

export type RowStatus = "ok" | "idle" | "taken" | "reserved" | null;

/** The core reports this error when it refuses a copy, paste or undo chord found in the settings file. */
const RESERVED_ERROR = "reserved";

function errorStatus(error: string | null): RowStatus {
  if (!error) return null;
  return error === RESERVED_ERROR ? "reserved" : "taken";
}

export function captureStatus(s: HotkeyStatus | undefined): RowStatus {
  if (!s) return null;
  if (s.captureRegistered) return "ok";
  return errorStatus(s.captureError);
}

/** Smart paste is registered only while there is something to paste, so "not registered" is normal. */
export function smartPasteStatus(s: HotkeyStatus | undefined): RowStatus {
  if (!s) return null;
  if (s.smartPasteRegistered) return "ok";
  return errorStatus(s.smartPasteError) ?? "idle";
}
