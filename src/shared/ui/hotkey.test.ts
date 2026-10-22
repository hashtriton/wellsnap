import { describe, expect, it } from "vitest";
import {
  acceleratorParts,
  defaultHotkeys,
  formatAccelerator,
  guessPlatform,
  isReservedAccelerator,
  keyFromCode,
  modifierLabel,
  normalizeAccelerator,
  recordKey,
  sameAccelerator,
  type KeyInput,
} from "./hotkey";

const key = (code: string, mods: Partial<Omit<KeyInput, "code">> = {}): KeyInput => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("recordKey", () => {
  it("maps physical keys and modifiers to the global-hotkey syntax", () => {
    expect(recordKey(key("KeyX", { ctrlKey: true, shiftKey: true }), "windows")).toEqual({
      kind: "ok",
      accelerator: "Ctrl+Shift+X",
    });
    expect(recordKey(key("KeyV", { altKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Alt+V" });
    expect(recordKey(key("Digit2", { metaKey: true, shiftKey: true }), "macos")).toEqual({
      kind: "ok",
      accelerator: "Shift+Cmd+2",
    });
    expect(recordKey(key("KeyS", { metaKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Super+S" });
    expect(recordKey(key("BracketLeft", { ctrlKey: true, altKey: true }), "windows")).toEqual({
      kind: "ok",
      accelerator: "Ctrl+Alt+BracketLeft",
    });
    expect(recordKey(key("Numpad5", { ctrlKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Ctrl+Numpad5" });
  });

  it("allows PrintScreen and F-keys without modifiers", () => {
    expect(recordKey(key("PrintScreen"), "windows")).toEqual({ kind: "ok", accelerator: "PrintScreen" });
    expect(recordKey(key("F8"), "windows")).toEqual({ kind: "ok", accelerator: "F8" });
    expect(recordKey(key("F24", { shiftKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Shift+F24" });
    expect(recordKey(key("PrintScreen", { ctrlKey: true }), "windows")).toEqual({
      kind: "ok",
      accelerator: "Ctrl+PrintScreen",
    });
  });

  it("requires Ctrl, Alt or Cmd for other keys", () => {
    expect(recordKey(key("KeyA"), "windows")).toEqual({ kind: "invalid", reason: "needsModifier" });
    expect(recordKey(key("KeyA", { shiftKey: true }), "windows")).toEqual({ kind: "invalid", reason: "needsModifier" });
    expect(recordKey(key("Space"), "macos")).toEqual({ kind: "invalid", reason: "needsModifier" });
  });

  it("rejects keys the parser does not know", () => {
    expect(recordKey(key("IntlBackslash", { ctrlKey: true }), "windows")).toEqual({
      kind: "invalid",
      reason: "unsupportedKey",
    });
    expect(recordKey(key("F25"), "windows")).toEqual({ kind: "invalid", reason: "unsupportedKey" });
    expect(recordKey(key("CapsLock", { ctrlKey: true }), "windows")).toEqual({ kind: "invalid", reason: "unsupportedKey" });
  });

  it("waits while only modifiers are held and cancels on Escape", () => {
    expect(recordKey(key("ControlLeft", { ctrlKey: true }), "windows")).toEqual({ kind: "pending", modifiers: ["Ctrl"] });
    expect(recordKey(key("MetaLeft", { metaKey: true, altKey: true }), "macos")).toEqual({
      kind: "pending",
      modifiers: ["Alt", "Cmd"],
    });
    expect(recordKey(key("Escape"), "windows")).toEqual({ kind: "cancel" });
    expect(recordKey(key("Escape", { ctrlKey: true }), "windows")).toEqual({ kind: "cancel" });
  });

  it("records NumpadEnter as the Enter that Windows really registers", () => {
    expect(recordKey(key("NumpadEnter", { ctrlKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Ctrl+Enter" });
    expect(recordKey(key("NumpadEnter", { ctrlKey: true }), "macos")).toEqual({ kind: "ok", accelerator: "Ctrl+NumpadEnter" });
  });

  it("refuses the clipboard and undo chords of the platform", () => {
    for (const code of ["KeyV", "KeyC", "KeyX", "KeyA", "KeyZ", "KeyY"]) {
      expect(recordKey(key(code, { ctrlKey: true }), "windows")).toEqual({ kind: "invalid", reason: "reserved" });
      expect(recordKey(key(code, { metaKey: true }), "macos")).toEqual({ kind: "invalid", reason: "reserved" });
    }
    // Other modifiers, or the other platform's command key, are fine.
    expect(recordKey(key("KeyV", { ctrlKey: true, shiftKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Ctrl+Shift+V" });
    expect(recordKey(key("KeyV", { ctrlKey: true }), "macos")).toEqual({ kind: "ok", accelerator: "Ctrl+V" });
    expect(recordKey(key("KeyV", { metaKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Super+V" });
    expect(recordKey(key("KeyV", { metaKey: true, altKey: true }), "windows")).toEqual({ kind: "ok", accelerator: "Alt+Super+V" });
  });

  it("knows reserved accelerators in any spelling", () => {
    expect(isReservedAccelerator("Control+KeyV", "windows")).toBe(true);
    expect(isReservedAccelerator("ctrl+z", "windows")).toBe(true);
    expect(isReservedAccelerator("Command+V", "macos")).toBe(true);
    expect(isReservedAccelerator("Cmd+V", "windows")).toBe(false);
    expect(isReservedAccelerator("Ctrl+Cmd+V", "macos")).toBe(false);
    expect(isReservedAccelerator("Super+Alt+V", "windows")).toBe(false);
    expect(isReservedAccelerator("Ctrl+B", "windows")).toBe(false);
  });

  it("maps codes to tokens", () => {
    expect(keyFromCode("KeyQ")).toBe("Q");
    expect(keyFromCode("Digit0")).toBe("0");
    expect(keyFromCode("F12")).toBe("F12");
    expect(keyFromCode("ArrowUp")).toBe("ArrowUp");
    expect(keyFromCode("Fn")).toBeNull();
    expect(keyFromCode("")).toBeNull();
  });
});

describe("accelerator comparison and display", () => {
  it("normalizes order, case and aliases", () => {
    expect(normalizeAccelerator("Shift+Cmd+2", "macos")).toBe("Shift+Meta+2");
    expect(sameAccelerator("Cmd+Shift+2", "shift+command+Digit2", "macos")).toBe(true);
    expect(sameAccelerator("Ctrl+Alt+V", "Alt+Control+KeyV", "windows")).toBe(true);
    expect(sameAccelerator("Ctrl+Alt+V", "Ctrl+V", "windows")).toBe(false);
    expect(sameAccelerator("Super+Alt+V", "Alt+Meta+V", "windows")).toBe(true);
    expect(sameAccelerator("PrintScreen", "printscreen", "windows")).toBe(true);
  });

  it("treats NumpadEnter as Enter on Windows only", () => {
    expect(sameAccelerator("Ctrl+NumpadEnter", "Ctrl+Enter", "windows")).toBe(true);
    expect(sameAccelerator("Ctrl+NumpadEnter", "Ctrl+Enter", "macos")).toBe(false);
    expect(formatAccelerator("Ctrl+NumpadEnter", "windows")).toBe("Ctrl+Enter");
  });

  it("formats for Windows", () => {
    expect(formatAccelerator("PrintScreen", "windows")).toBe("PrtSc");
    expect(formatAccelerator("Ctrl+Alt+V", "windows")).toBe("Ctrl+Alt+V");
    expect(formatAccelerator("Super+Alt+V", "windows")).toBe("Win+Alt+V");
    expect(formatAccelerator("Alt+Super+V", "windows")).toBe("Win+Alt+V");
    expect(formatAccelerator("Shift+Super+S", "windows")).toBe("Win+Shift+S");
    expect(acceleratorParts("Shift+Alt+Ctrl+Super+K", "windows")).toEqual(["Win", "Ctrl", "Alt", "Shift", "K"]);
    expect(formatAccelerator("Ctrl+ArrowUp", "windows")).toBe("Ctrl+↑");
    expect(acceleratorParts("Ctrl+Numpad5", "windows")).toEqual(["Ctrl", "Num 5"]);
    expect(acceleratorParts("Alt+Shift+BracketLeft", "windows")).toEqual(["Alt", "Shift", "["]);
  });

  it("formats for macOS with symbols in Apple order", () => {
    expect(formatAccelerator("Cmd+Shift+2", "macos")).toBe("⇧⌘2");
    expect(formatAccelerator("Ctrl+Cmd+V", "macos")).toBe("⌃⌘V");
    expect(acceleratorParts("Alt+Cmd+Enter", "macos")).toEqual(["⌥", "⌘", "↩"]);
  });

  it("labels single modifiers for the live recorder", () => {
    expect(modifierLabel("Cmd", "macos")).toBe("⌘");
    expect(modifierLabel("Super", "windows")).toBe("Win");
    expect(modifierLabel("Alt", "macos")).toBe("⌥");
    expect(modifierLabel("Ctrl", "windows")).toBe("Ctrl");
  });

  it("handles empty input", () => {
    expect(acceleratorParts("", "windows")).toEqual([]);
    expect(formatAccelerator("", "macos")).toBe("");
  });

  it("knows the platform defaults", () => {
    expect(defaultHotkeys("windows")).toEqual({ capture: "PrintScreen", smartPaste: "Super+Alt+V" });
    expect(formatAccelerator(defaultHotkeys("windows").smartPaste, "windows")).toBe("Win+Alt+V");
    expect(isReservedAccelerator(defaultHotkeys("windows").smartPaste, "windows")).toBe(false);
    expect(isReservedAccelerator(defaultHotkeys("macos").smartPaste, "macos")).toBe(false);
    expect(defaultHotkeys("macos")).toEqual({ capture: "Cmd+Shift+2", smartPaste: "Ctrl+Cmd+V" });
  });

  it("guesses the platform from the user agent", () => {
    expect(guessPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/141.0")).toBe("windows");
    expect(guessPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")).toBe("macos");
    expect(guessPlatform("Mozilla/5.0 (X11; Linux x86_64)")).toBe("other");
  });
});
