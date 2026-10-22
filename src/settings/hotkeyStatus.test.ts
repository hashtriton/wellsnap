import { describe, expect, it } from "vitest";
import type { HotkeyStatus } from "../shared/ipc";
import { captureStatus, smartPasteStatus } from "./hotkeyStatus";

const status = (patch: Partial<HotkeyStatus>): HotkeyStatus => ({
  captureRegistered: false,
  captureError: null,
  captureErrorKind: null,
  smartPasteRegistered: false,
  smartPasteError: null,
  smartPasteErrorKind: null,
  ...patch,
});

describe("hotkey status", () => {
  it("shows registered, taken and reserved capture hotkeys by the error kind", () => {
    expect(captureStatus(undefined)).toBeNull();
    expect(captureStatus(status({ captureRegistered: true }))).toBe("ok");
    expect(captureStatus(status({ captureError: "HotKey already registered: PrintScreen", captureErrorKind: "taken" }))).toBe("taken");
    expect(captureStatus(status({ captureError: "reserved", captureErrorKind: "reserved" }))).toBe("reserved");
    expect(captureStatus(status({ captureError: "Unable to register hotkey", captureErrorKind: "other" }))).toBe("taken");
    expect(captureStatus(status({}))).toBeNull();
  });

  it("prefers the kind to the text of the error", () => {
    expect(captureStatus(status({ captureError: "reserved", captureErrorKind: "taken" }))).toBe("taken");
    expect(captureStatus(status({ captureError: "something", captureErrorKind: "reserved" }))).toBe("reserved");
  });

  it("still knows the reserved text from a core that sends no kind", () => {
    expect(captureStatus(status({ captureError: "reserved" }))).toBe("reserved");
    expect(captureStatus(status({ captureError: "HotKey already registered: PrintScreen" }))).toBe("taken");
    expect(smartPasteStatus(status({ smartPasteError: "reserved" }))).toBe("reserved");
    expect(smartPasteStatus(status({ smartPasteError: "taken by another app" }))).toBe("taken");
  });

  it("treats an unregistered smart paste without an error as idle", () => {
    expect(smartPasteStatus(status({}))).toBe("idle");
    expect(smartPasteStatus(status({ smartPasteRegistered: true }))).toBe("ok");
    expect(smartPasteStatus(status({ smartPasteError: "reserved", smartPasteErrorKind: "reserved" }))).toBe("reserved");
    expect(smartPasteStatus(status({ smartPasteError: "taken by another app", smartPasteErrorKind: "taken" }))).toBe("taken");
    expect(smartPasteStatus(status({ smartPasteError: "failed", smartPasteErrorKind: "other" }))).toBe("taken");
  });
});
