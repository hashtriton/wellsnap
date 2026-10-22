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
  it("shows registered, taken and reserved capture hotkeys", () => {
    expect(captureStatus(undefined)).toBeNull();
    expect(captureStatus(status({ captureRegistered: true }))).toBe("ok");
    expect(captureStatus(status({ captureError: "HotKey already registered" }))).toBe("taken");
    expect(captureStatus(status({ captureError: "reserved" }))).toBe("reserved");
    expect(captureStatus(status({}))).toBeNull();
  });

  it("treats an unregistered smart paste without an error as idle", () => {
    expect(smartPasteStatus(status({}))).toBe("idle");
    expect(smartPasteStatus(status({ smartPasteRegistered: true }))).toBe("ok");
    expect(smartPasteStatus(status({ smartPasteError: "reserved" }))).toBe("reserved");
    expect(smartPasteStatus(status({ smartPasteError: "taken by another app" }))).toBe("taken");
  });
});
