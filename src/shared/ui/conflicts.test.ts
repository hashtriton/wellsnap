import { describe, expect, it } from "vitest";
import { en } from "../../locales/en";
import { ru } from "../../locales/ru";
import { createT } from "../i18n";
import type { ConflictReport, ContenderId, ContenderRef, HotkeyStatus, PrintScreenStatus } from "../ipc";
import {
  alternativeKey,
  blockedText,
  captureAdvice,
  doubleNotes,
  fixSteps,
  isTaken,
  joinNames,
  stepsKey,
  suspectsOf,
  takenText,
  type AdviceInput,
  type CaptureAdvice,
} from "./conflicts";

const status = (patch: Partial<HotkeyStatus> = {}): HotkeyStatus => ({
  captureRegistered: false,
  captureError: null,
  captureErrorKind: null,
  smartPasteRegistered: false,
  smartPasteError: null,
  smartPasteErrorKind: null,
  ...patch,
});
const taken = status({ captureError: "HotKey already registered: PrintScreen", captureErrorKind: "taken" });

const report = (patch: Partial<ConflictReport> = {}): ConflictReport => ({ running: [], suspects: [], mayDouble: [], alternatives: [], ...patch });
const printScreen = (patch: Partial<PrintScreenStatus> = {}): PrintScreenStatus => ({
  applicable: true,
  snippingEnabled: false,
  onedriveEnabled: false,
  changedByUs: false,
  ...patch,
});

const shareX: ContenderRef = { id: "shareX", name: "ShareX" };
const lightshot: ContenderRef = { id: "lightshot", name: "Lightshot" };
const gyazo: ContenderRef = { id: "gyazo", name: "Gyazo" };
const oneDrive: ContenderRef = { id: "oneDrive", name: "OneDrive" };

const input = (patch: Partial<AdviceInput> = {}): AdviceInput => ({
  hotkey: "PrintScreen",
  status: taken,
  printScreen: printScreen(),
  report: report(),
  platform: "windows",
  ...patch,
});

describe("captureAdvice", () => {
  it("knows nothing before the status arrives or while the recorder suspends the keys", () => {
    expect(captureAdvice(input({ status: undefined }))).toBeNull();
    expect(captureAdvice(input({ status: status() }))).toBeNull();
  });

  it("keeps the Snipping Tool case first, as before", () => {
    const snipping = printScreen({ snippingEnabled: true });
    expect(captureAdvice(input({ printScreen: snipping, report: report({ suspects: [shareX] }) }))).toEqual({ kind: "snipping" });
    expect(captureAdvice(input({ printScreen: snipping, status: status({ captureRegistered: true }) }))).toEqual({ kind: "snipping" });
    expect(captureAdvice(input({ printScreen: snipping, status: undefined }))).toEqual({ kind: "snipping" });
    expect(captureAdvice(input({ printScreen: snipping, hotkey: "printscreen" }))).toEqual({ kind: "snipping" });
    // Another key, Windows 10 or macOS: Snipping Tool is not involved.
    expect(captureAdvice(input({ printScreen: snipping, hotkey: "Ctrl+PrintScreen" }))?.kind).toBe("takenUnknown");
    expect(captureAdvice(input({ printScreen: printScreen({ applicable: false, snippingEnabled: true }) }))?.kind).toBe("takenUnknown");
    expect(captureAdvice(input({ printScreen: snipping, platform: "macos" }))?.kind).toBe("other");
  });

  it("is ok when registered, or names the tools that may fire as well", () => {
    const ok = status({ captureRegistered: true });
    expect(captureAdvice(input({ status: ok }))).toEqual({ kind: "ok" });
    expect(captureAdvice(input({ status: ok, report: null }))).toEqual({ kind: "ok" });
    expect(captureAdvice(input({ status: ok, report: report({ mayDouble: [gyazo] }) }))).toEqual({ kind: "mayDouble", apps: [gyazo] });
    expect(captureAdvice(input({ status: ok, platform: "macos", report: report({ mayDouble: [gyazo] }) }))).toEqual({ kind: "ok" });
  });

  it("names one owner, several owners or none of a taken key", () => {
    expect(captureAdvice(input({ report: report({ suspects: [shareX] }) }))).toEqual({ kind: "takenBy", app: shareX });
    expect(captureAdvice(input({ report: report({ suspects: [shareX, lightshot] }) }))).toEqual({
      kind: "takenMany",
      apps: [shareX, lightshot],
    });
    expect(captureAdvice(input({ report: report() }))).toEqual({ kind: "takenUnknown" });
    expect(captureAdvice(input({ report: null }))).toEqual({ kind: "takenUnknown" });
  });

  it("keeps reserved and other failures apart, and macOS on its usual texts", () => {
    expect(captureAdvice(input({ status: status({ captureError: "reserved", captureErrorKind: "reserved" }) }))).toEqual({ kind: "reserved" });
    expect(captureAdvice(input({ status: status({ captureError: "bad", captureErrorKind: "other" }) }))).toEqual({ kind: "other" });
    expect(captureAdvice(input({ status: status({ captureError: "no kind yet" }) }))).toEqual({ kind: "other" });
    expect(captureAdvice(input({ platform: "macos", report: report({ suspects: [shareX] }) }))).toEqual({ kind: "other" });
  });

  it("marks the taken cases", () => {
    const all: (CaptureAdvice | null)[] = [
      null,
      { kind: "ok" },
      { kind: "snipping" },
      { kind: "takenBy", app: shareX },
      { kind: "takenMany", apps: [shareX, lightshot] },
      { kind: "takenUnknown" },
      { kind: "reserved" },
      { kind: "mayDouble", apps: [gyazo] },
      { kind: "other" },
    ];
    expect(all.filter(isTaken).map((a) => a.kind)).toEqual(["takenBy", "takenMany", "takenUnknown"]);
  });
});

describe("joinNames", () => {
  it("joins with commas and the last one with or", () => {
    const tRu = createT("ru");
    const tEn = createT("en");
    expect(joinNames([], tRu)).toBe("");
    expect(joinNames(["ShareX"], tRu)).toBe("ShareX");
    expect(joinNames(["ShareX", "Lightshot"], tRu)).toBe("ShareX или Lightshot");
    expect(joinNames(["ShareX", "Lightshot", "Dropbox"], tRu)).toBe("ShareX, Lightshot или Dropbox");
    expect(joinNames(["ShareX", "Lightshot"], tEn)).toBe("ShareX or Lightshot");
    expect(joinNames(["ShareX", "Lightshot", "Dropbox"], tEn)).toBe("ShareX, Lightshot or Dropbox");
  });
});

describe("texts", () => {
  const t = createT("en");

  it("name the owner in the status line", () => {
    expect(takenText({ kind: "takenBy", app: shareX }, "PrtSc", t)).toBe(
      'Not active: ShareX is using PrtSc. Turn off PrtSc in ShareX or quit it, then click "Check again".',
    );
    expect(takenText({ kind: "takenMany", apps: [shareX, lightshot] }, "PrtSc", t)).toBe(
      "Not active: another app is using PrtSc. Most likely ShareX or Lightshot.",
    );
    expect(takenText({ kind: "takenUnknown" }, "PrtSc", t)).toBeNull();
    expect(takenText({ kind: "other" }, "PrtSc", t)).toBeNull();
    expect(takenText(null, "PrtSc", t)).toBeNull();
  });

  it("tell onboarding why the key does not reach WellSnap", () => {
    const tRu = createT("ru");
    expect(blockedText({ kind: "snipping" }, "PrtSc", t)).toContain("Snipping Tool");
    expect(blockedText({ kind: "takenBy", app: shareX }, "PrtSc", t)).toMatch(/^PrtSc is in use by ShareX right now/);
    expect(blockedText({ kind: "takenMany", apps: [shareX, lightshot] }, "PrtSc", tRu)).toMatch(
      /^Сейчас PrtSc занята программой ShareX или Lightshot,/,
    );
    expect(blockedText({ kind: "takenUnknown" }, "PrtSc", t)).toMatch(/^Another app is using PrtSc right now/);
    for (const kind of ["ok", "reserved", "other"] as const) expect(blockedText({ kind }, "PrtSc", t)).toBeNull();
    expect(blockedText({ kind: "mayDouble", apps: [gyazo] }, "PrtSc", t)).toBeNull();
    expect(blockedText(null, "PrtSc", t)).toBeNull();
  });
});

describe("fix steps", () => {
  const t = createT("en");

  it("come from the suspects, with OneDrive's own Print Screen text", () => {
    expect(suspectsOf({ kind: "takenBy", app: shareX })).toEqual([shareX]);
    expect(suspectsOf({ kind: "takenMany", apps: [shareX, oneDrive] })).toEqual([shareX, oneDrive]);
    expect(suspectsOf({ kind: "takenUnknown" })).toEqual([]);
    expect(stepsKey("oneDrive")).toBe("settings.printScreen.onedrive");
    expect(stepsKey("shareX")).toBe("settings.conflicts.steps.shareX");
    expect(fixSteps([shareX, oneDrive], null, t)).toEqual([en.settings.conflicts.steps.shareX, en.settings.printScreen.onedrive]);
  });

  it("leave OneDrive out while PrintScreenCard shows its notice", () => {
    expect(fixSteps([shareX, oneDrive], printScreen({ onedriveEnabled: true }), t)).toEqual([en.settings.conflicts.steps.shareX]);
    // Windows 10: PrintScreenCard is hidden, so the card keeps OneDrive.
    expect(fixSteps([oneDrive], printScreen({ applicable: false, onedriveEnabled: true }), t)).toEqual([en.settings.printScreen.onedrive]);
  });

  it("exist for every contender in Russian and English", () => {
    // Exhaustive: a new ContenderId fails to compile here until it is listed.
    const ids = Object.keys({
      oneDrive: 1,
      dropbox: 1,
      lightshot: 1,
      yandexDisk: 1,
      shareX: 1,
      greenshot: 1,
      flameshot: 1,
      snagit: 1,
      picPick: 1,
      gyazo: 1,
      screenpresso: 1,
      fastStone: 1,
      joxi: 1,
    } satisfies Record<ContenderId, 1>) as ContenderId[];
    for (const dict of [ru, en]) {
      const steps: Record<string, string> = dict.settings.conflicts.steps;
      for (const id of ids.filter((i) => i !== "oneDrive")) expect(steps[id]?.trim(), id).toBeTruthy();
      expect(Object.keys(steps).sort()).toEqual(ids.filter((i) => i !== "oneDrive").sort());
      expect(dict.settings.printScreen.onedrive.trim()).toBeTruthy();
    }
    for (const lang of ["ru", "en"] as const) {
      const tl = createT(lang);
      for (const id of ids) expect(tl(stepsKey(id)), `${lang} ${id}`).not.toBe(stepsKey(id));
    }
  });
});

describe("double-fire notes", () => {
  const may: CaptureAdvice = { kind: "mayDouble", apps: [gyazo, oneDrive] };

  it("drop the hidden ones and the OneDrive one PrintScreenCard shows", () => {
    expect(doubleNotes(may, printScreen(), [])).toEqual([gyazo, oneDrive]);
    expect(doubleNotes(may, printScreen(), ["gyazo"])).toEqual([oneDrive]);
    expect(doubleNotes(may, printScreen({ onedriveEnabled: true }), [])).toEqual([gyazo]);
    expect(doubleNotes({ kind: "takenBy", app: shareX }, printScreen(), [])).toEqual([]);
    expect(doubleNotes(null, null, [])).toEqual([]);
  });
});

describe("alternativeKey", () => {
  const takenBy: CaptureAdvice = { kind: "takenBy", app: shareX };

  it("offers the first free key only for a taken key", () => {
    const r = report({ alternatives: ["Shift+PrintScreen", "Ctrl+Alt+PrintScreen"] });
    expect(alternativeKey(takenBy, r, "PrintScreen", "windows")).toBe("Shift+PrintScreen");
    expect(alternativeKey({ kind: "takenUnknown" }, r, "PrintScreen", "windows")).toBe("Shift+PrintScreen");
    expect(alternativeKey({ kind: "ok" }, r, "PrintScreen", "windows")).toBeNull();
    expect(alternativeKey({ kind: "other" }, r, "PrintScreen", "windows")).toBeNull();
    expect(alternativeKey(takenBy, null, "PrintScreen", "windows")).toBeNull();
  });

  it("never offers the current key and shows no chip when nothing is left", () => {
    const r = report({ alternatives: ["Shift+PrintScreen", "Ctrl+Alt+PrintScreen"] });
    expect(alternativeKey(takenBy, r, "shift+printscreen", "windows")).toBe("Ctrl+Alt+PrintScreen");
    expect(alternativeKey(takenBy, report({ alternatives: ["Shift+PrintScreen"] }), "Shift+PrintScreen", "windows")).toBeNull();
    expect(alternativeKey(takenBy, report(), "PrintScreen", "windows")).toBeNull();
  });
});
