// Accelerators in the global-hotkey syntax used by the Rust side ("Ctrl+Shift+X", "Cmd+Shift+2",
// "PrintScreen", "F8"): recording from keyboard events, comparison and display.
import type { Platform } from "../ipc";

export interface KeyInput {
  /** KeyboardEvent.code: the physical key, independent of the keyboard layout. */
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type InvalidReason = "needsModifier" | "unsupportedKey" | "reserved";

export type RecordResult =
  | { kind: "pending"; modifiers: string[] }
  | { kind: "cancel" }
  | { kind: "ok"; accelerator: string }
  | { kind: "invalid"; reason: InvalidReason };

const MODIFIER_CODES = new Set([
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "ShiftLeft",
  "ShiftRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
]);

// event.code values that the global-hotkey parser accepts under the same name.
const NAMED_KEYS = new Set([
  "Space",
  "Enter",
  "Backspace",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "PrintScreen",
  "ScrollLock",
  "Pause",
  "Backquote",
  "Minus",
  "Equal",
  "BracketLeft",
  "BracketRight",
  "Backslash",
  "Semicolon",
  "Quote",
  "Comma",
  "Period",
  "Slash",
  "NumpadAdd",
  "NumpadSubtract",
  "NumpadMultiply",
  "NumpadDivide",
  "NumpadDecimal",
  "NumpadEnter",
]);

const F_KEY = /^F([1-9]|1[0-9]|2[0-4])$/;

/** Accelerator token for a physical key, or null when the key cannot be a global hotkey. */
export function keyFromCode(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^Numpad[0-9]$/.test(code) || F_KEY.test(code) || NAMED_KEYS.has(code)) return code;
  return null;
}

function modifierTokens(input: KeyInput, platform: Platform): string[] {
  const mods: string[] = [];
  if (input.ctrlKey) mods.push("Ctrl");
  if (input.altKey) mods.push("Alt");
  if (input.shiftKey) mods.push("Shift");
  if (input.metaKey) mods.push(platform === "macos" ? "Cmd" : "Super");
  return mods;
}

/**
 * Turns a key press into an accelerator. PrintScreen and F-keys may be used alone (with or without
 * modifiers); every other key needs Ctrl, Alt or Cmd, because Shift alone would swallow normal typing.
 * The clipboard and undo chords of the platform are refused: a global Ctrl+V would also catch the
 * synthetic paste that smart paste sends.
 */
export function recordKey(input: KeyInput, platform: Platform): RecordResult {
  if (input.code === "Escape") return { kind: "cancel" };
  const mods = modifierTokens(input, platform);
  if (MODIFIER_CODES.has(input.code) || input.code === "") return { kind: "pending", modifiers: mods };
  let key = keyFromCode(input.code);
  if (!key) return { kind: "invalid", reason: "unsupportedKey" };
  // Windows registers NumpadEnter as plain Enter (VK_RETURN); record what will really be registered.
  if (key === "NumpadEnter" && platform !== "macos") key = "Enter";
  const standalone = key === "PrintScreen" || F_KEY.test(key);
  if (!standalone && !(input.ctrlKey || input.altKey || input.metaKey)) {
    return { kind: "invalid", reason: "needsModifier" };
  }
  const accelerator = [...mods, key].join("+");
  if (isReservedAccelerator(accelerator, platform)) return { kind: "invalid", reason: "reserved" };
  return { kind: "ok", accelerator };
}

const MOD_ORDER = ["Ctrl", "Alt", "Shift", "Meta"];

function canonicalModifier(token: string): string | null {
  switch (token.toLowerCase()) {
    case "ctrl":
    case "control":
      return "Ctrl";
    case "alt":
    case "option":
      return "Alt";
    case "shift":
      return "Shift";
    case "cmd":
    case "command":
    case "super":
    case "meta":
      return "Meta";
    default:
      return null;
  }
}

function canonicalKey(token: string, platform: Platform): string {
  const t = token.trim();
  const letter = /^(?:Key)?([A-Za-z])$/i.exec(t);
  if (letter) return letter[1].toUpperCase();
  const digit = /^(?:Digit)?([0-9])$/i.exec(t);
  if (digit) return digit[1];
  const upper = t.toUpperCase();
  // Same key for the OS on Windows (VK_RETURN), a separate one on macOS.
  return upper === "NUMPADENTER" && platform !== "macos" ? "ENTER" : upper;
}

/** Canonical form for comparing two accelerators ("cmd+shift+2" equals "Shift+Command+Digit2"). */
export function normalizeAccelerator(accel: string, platform: Platform): string {
  const parts = accel.split("+").map((p) => p.trim());
  const mods = new Set<string>();
  let key = "";
  for (const part of parts) {
    const mod = canonicalModifier(part);
    if (mod) mods.add(mod);
    else key = canonicalKey(part, platform);
  }
  return [...MOD_ORDER.filter((m) => mods.has(m)), key].join("+");
}

export function sameAccelerator(a: string, b: string, platform: Platform): boolean {
  return normalizeAccelerator(a, platform) === normalizeAccelerator(b, platform);
}

const RESERVED_KEYS = ["V", "C", "X", "A", "Z", "Y"];

/** Paste, copy, cut, select all, undo and redo with the platform's command modifier (Ctrl, on macOS Cmd). */
export function isReservedAccelerator(accel: string, platform: Platform): boolean {
  const command = platform === "macos" ? "Meta" : "Ctrl";
  const normalized = normalizeAccelerator(accel, platform);
  return RESERVED_KEYS.some((key) => normalized === `${command}+${key}`);
}

export function defaultHotkeys(platform: Platform): { capture: string; smartPaste: string } {
  // Not Ctrl+Alt+V on Windows: Office uses it for Paste Special.
  return platform === "macos"
    ? { capture: "Cmd+Shift+2", smartPaste: "Ctrl+Cmd+V" }
    : { capture: "PrintScreen", smartPaste: "Super+Alt+V" };
}

const KEY_LABELS: Record<string, string> = {
  PRINTSCREEN: "PrtSc",
  SPACE: "Space",
  ENTER: "Enter",
  BACKSPACE: "Backspace",
  DELETE: "Del",
  INSERT: "Ins",
  HOME: "Home",
  END: "End",
  PAGEUP: "PgUp",
  PAGEDOWN: "PgDn",
  ARROWUP: "↑",
  ARROWDOWN: "↓",
  ARROWLEFT: "←",
  ARROWRIGHT: "→",
  UP: "↑",
  DOWN: "↓",
  LEFT: "←",
  RIGHT: "→",
  SCROLLLOCK: "ScrLk",
  PAUSE: "Pause",
  ESCAPE: "Esc",
  ESC: "Esc",
  TAB: "Tab",
  BACKQUOTE: "`",
  MINUS: "-",
  EQUAL: "=",
  BRACKETLEFT: "[",
  BRACKETRIGHT: "]",
  BACKSLASH: "\\",
  SEMICOLON: ";",
  QUOTE: "'",
  COMMA: ",",
  PERIOD: ".",
  SLASH: "/",
  NUMPADADD: "Num +",
  NUMPADSUBTRACT: "Num -",
  NUMPADMULTIPLY: "Num *",
  NUMPADDIVIDE: "Num /",
  NUMPADDECIMAL: "Num .",
  NUMPADENTER: "Num Enter",
};

const MAC_KEY_LABELS: Record<string, string> = { ENTER: "↩", BACKSPACE: "⌫", DELETE: "⌦", ESCAPE: "⎋", ESC: "⎋" };

function keyLabel(key: string, platform: Platform): string {
  if (/^[A-Z0-9]$/.test(key) || /^F[0-9]{1,2}$/.test(key)) return key;
  const numpad = /^(?:NUMPAD|NUM)([0-9])$/.exec(key);
  if (numpad) return `Num ${numpad[1]}`;
  return (platform === "macos" ? MAC_KEY_LABELS[key] : undefined) ?? KEY_LABELS[key] ?? key;
}

// Apple order on macOS (⌃⌥⇧⌘); the Windows key comes first elsewhere, as in "Win+Shift+S".
const DISPLAY_ORDER: Record<Platform, string[]> = {
  macos: MOD_ORDER,
  windows: ["Meta", "Ctrl", "Alt", "Shift"],
  other: ["Meta", "Ctrl", "Alt", "Shift"],
};

/** Key caps for display: ["Win", "Alt", "V"], ["PrtSc"], on macOS ["⌃", "⌘", "V"]. */
export function acceleratorParts(accel: string, platform: Platform): string[] {
  const normalized = normalizeAccelerator(accel, platform);
  if (!normalized) return [];
  const tokens = normalized.split("+");
  const key = tokens.pop() ?? "";
  const parts = DISPLAY_ORDER[platform].filter((m) => tokens.includes(m)).map((m) => modifierLabel(m, platform));
  if (key) parts.push(keyLabel(key, platform));
  return parts;
}

/** Display label of a modifier token ("Ctrl", "Cmd", "Super", ...): "Win" on Windows, symbols on macOS. */
export function modifierLabel(token: string, platform: Platform): string {
  const mod = canonicalModifier(token) ?? token;
  const labels: Record<string, string> =
    platform === "macos"
      ? { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘" }
      : { Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift", Meta: "Win" };
  return labels[mod] ?? token;
}

/** One-line form: "Win+Alt+V", "PrtSc", on macOS "⌃⌘V". */
export function formatAccelerator(accel: string, platform: Platform): string {
  return acceleratorParts(accel, platform).join(platform === "macos" ? "" : "+");
}

/** Platform before `get_app_info` answers (WebView2 on Windows, WKWebView on macOS). */
export function guessPlatform(userAgent: string): Platform {
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "macos";
  if (/Windows/i.test(userAgent)) return "windows";
  return "other";
}
