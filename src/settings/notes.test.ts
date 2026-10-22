import { describe, expect, it } from "vitest";
import type { UpdateStatus } from "../shared/ipc";
import { formatDate, pickNotes, stateAfterInstallError, updateView } from "./notes";

const BOTH = [
  "WellSnap 0.2.0",
  "",
  "### Русский",
  "- Обновления ставятся сами",
  "#### Подробнее",
  "Текст",
  "",
  "### English",
  "- Updates install themselves",
  "",
  "## Downloads",
  "Windows and macOS",
].join("\n");

describe("pickNotes", () => {
  it("picks the section of the language and drops text outside the sections", () => {
    expect(pickNotes(BOTH, "ru")).toBe("- Обновления ставятся сами\n#### Подробнее\nТекст");
    expect(pickNotes(BOTH, "en")).toBe("- Updates install themselves");
  });

  it("falls back to the other section when one is missing or empty", () => {
    const enOnly = "### English\nFixes";
    expect(pickNotes(enOnly, "ru")).toBe("Fixes");
    expect(pickNotes(enOnly, "en")).toBe("Fixes");
    expect(pickNotes("### Русский\n\n### English\nFixes", "ru")).toBe("Fixes");
  });

  it("returns the whole text without headings, and null for nothing", () => {
    expect(pickNotes("  Just fixes.\r\nMore.  ", "ru")).toBe("Just fixes.\r\nMore.");
    expect(pickNotes("## Other\ntext", "en")).toBe("## Other\ntext");
    expect(pickNotes("", "en")).toBeNull();
    expect(pickNotes("   ", "en")).toBeNull();
    expect(pickNotes(null, "en")).toBeNull();
  });

  it("drops HTML comments and shows nothing when both sections are empty", () => {
    const template = "<!--\nCopy to release-notes/vX.Y.Z.md\n-->\n\n### Русский\n\n### English\n";
    expect(pickNotes(template, "ru")).toBeNull();
    expect(pickNotes(template, "en")).toBeNull();
    expect(pickNotes("<!-- note -->\n### English\n- Fixed", "ru")).toBe("- Fixed");
    expect(pickNotes("<!-- a -->Fixes<!-- b -->", "en")).toBe("Fixes");
    expect(pickNotes("<!-- only a comment -->", "en")).toBeNull();
    expect(pickNotes("### Русский\n## Downloads\nWindows", "ru")).toBeNull();
  });

  it("accepts headings in any case and with closing hashes", () => {
    expect(pickNotes("## ENGLISH ##\nA\n### русский\nБ", "en")).toBe("A");
    expect(pickNotes("## ENGLISH ##\nA\n### русский\nБ", "ru")).toBe("Б");
  });
});

describe("updateView", () => {
  const on = { autoCheckUpdates: true, autoInstallUpdates: true };
  const status = (phase: UpdateStatus["phase"], autoEligible = true): UpdateStatus => ({
    phase,
    info: phase === "idle" ? null : { version: "0.2.0", notes: null },
    autoEligible,
    recent: null,
  });

  it("maps the core phase and the page's own state", () => {
    const cases: [UpdateStatus | null, Parameters<typeof updateView>[1], typeof on, ReturnType<typeof updateView>][] = [
      [null, "idle", on, { kind: "idle" }],
      [status("idle"), "checking", on, { kind: "checking" }],
      [status("idle"), "latest", on, { kind: "latest" }],
      [status("idle"), "error", on, { kind: "error" }],
      [status("idle"), "installing", on, { kind: "checking" }],
      [status("available"), "idle", on, { kind: "available", version: "0.2.0", auto: true, failed: false, busy: false }],
      [status("available"), "error", on, { kind: "available", version: "0.2.0", auto: true, failed: true, busy: false }],
      [status("available", false), "idle", on, { kind: "available", version: "0.2.0", auto: false, failed: false, busy: false }],
      [status("available"), "idle", { ...on, autoInstallUpdates: false }, { kind: "available", version: "0.2.0", auto: false, failed: false, busy: false }],
      [status("available"), "idle", { ...on, autoCheckUpdates: false }, { kind: "available", version: "0.2.0", auto: false, failed: false, busy: false }],
      [status("available"), "busy", on, { kind: "available", version: "0.2.0", auto: true, failed: false, busy: true }],
      [status("idle"), "busy", on, { kind: "idle" }],
      [status("available"), "installing", on, { kind: "installing", version: "0.2.0" }],
      [status("installing"), "idle", on, { kind: "installing", version: "0.2.0" }],
    ];
    for (const [s, local, settings, view] of cases) expect(updateView(s, local, settings)).toEqual(view);
  });
});

describe("stateAfterInstallError", () => {
  it("maps the core's refusal codes and treats anything else as a failure", () => {
    expect(stateAfterInstallError("busy")).toBe("busy");
    expect(stateAfterInstallError("gone")).toBe("latest");
    expect(stateAfterInstallError("error sending request for url")).toBe("error");
    expect(stateAfterInstallError(new Error("busy"))).toBe("error");
    expect(stateAfterInstallError(undefined)).toBe("error");
  });
});

describe("formatDate", () => {
  it("formats Unix seconds in the UI language", () => {
    const at = Date.UTC(2026, 9, 21, 12) / 1000;
    expect(formatDate(at, "en")).toContain("2026");
    expect(formatDate(at, "en")).toContain("October");
    expect(formatDate(at, "ru")).toContain("октября");
  });
});
