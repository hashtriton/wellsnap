import { describe, expect, it } from "vitest";
import { createT, resolveLang } from "./i18n";

describe("i18n", () => {
  it("uses Russian plural forms", () => {
    const t = createT("ru");
    expect(t("common.snapshots", { count: 1 })).toBe("1 снимок");
    expect(t("common.snapshots", { count: 3 })).toBe("3 снимка");
    expect(t("common.snapshots", { count: 5 })).toBe("5 снимков");
    expect(t("common.snapshots", { count: 11 })).toBe("11 снимков");
    expect(t("common.snapshots", { count: 21 })).toBe("21 снимок");
    expect(t("common.comments", { count: 22 })).toBe("22 комментария");
  });

  it("uses English plural forms", () => {
    const t = createT("en");
    expect(t("common.snapshots", { count: 1 })).toBe("1 screenshot");
    expect(t("common.snapshots", { count: 2 })).toBe("2 screenshots");
  });

  it("falls back to the key for unknown strings", () => {
    expect(createT("ru")("nope.missing")).toBe("nope.missing");
  });

  it("resolves the language from preference and locale", () => {
    expect(resolveLang("en", "ru-RU")).toBe("en");
    expect(resolveLang("auto", "ru-RU")).toBe("ru");
    expect(resolveLang("auto", "kk")).toBe("ru");
    expect(resolveLang("auto", "de-DE")).toBe("en");
    expect(resolveLang("auto", undefined)).toBe("en");
  });
});
