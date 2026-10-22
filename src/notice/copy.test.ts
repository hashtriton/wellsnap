import { describe, expect, it } from "vitest";
import { createT } from "../shared/i18n";
import { crashCopy, fileName, nudgeCopy, toastDetail } from "./copy";

describe("nudgeCopy", () => {
  const ru = createT("ru");
  const en = createT("en");

  it("picks one of four variants by index", () => {
    const titles = [0, 1, 2, 3].map((i) => nudgeCopy(ru, i, 300).title);
    expect(new Set(titles).size).toBe(4);
    expect(titles[0]).toBe("Приложение пригодилось?");
    expect(titles[3]).toBe("Вы с нами давно");
  });

  it("clamps out-of-range indexes", () => {
    expect(nudgeCopy(ru, 7, 5000)).toEqual(nudgeCopy(ru, 3, 5000));
    expect(nudgeCopy(ru, -1, 50)).toEqual(nudgeCopy(ru, 0, 50));
    expect(nudgeCopy(ru, Number.NaN, 50)).toEqual(nudgeCopy(ru, 0, 50));
  });

  it("uses Russian plural forms for the count", () => {
    expect(nudgeCopy(ru, 0, 50).body).toContain("Вы уже сделали 50 скриншотов.");
    expect(nudgeCopy(ru, 0, 51).body).toContain("Вы уже сделали 51 скриншот.");
    expect(nudgeCopy(ru, 1, 302).title).toBe("302 скриншота позади");
    expect(nudgeCopy(ru, 2, 1000).title).toBe("Уже 1000 скриншотов");
  });

  it("has complete English copy", () => {
    expect(nudgeCopy(en, 0, 50).title).toBe("Enjoying WellSnap?");
    expect(nudgeCopy(en, 0, 1).body).toContain("You have taken 1 screenshot.");
    expect(nudgeCopy(en, 1, 300).title).toBe("300 screenshots and counting");
  });

  it("never leaves placeholders or long dashes", () => {
    for (const t of [ru, en]) {
      for (let i = 0; i < 4; i++) {
        const { title, body } = nudgeCopy(t, i, 300);
        expect(`${title} ${body}`).not.toMatch(/[{}\u2012\u2013\u2014]|notice\./);
      }
    }
  });
});

describe("crashCopy", () => {
  it("has the prompt and its four buttons in both languages", () => {
    expect(crashCopy(createT("ru")).title).toBe("WellSnap неожиданно закрылся");
    expect(crashCopy(createT("en")).always).toBe("Always send without asking");
    for (const lang of ["ru", "en"] as const) {
      const texts = Object.values(crashCopy(createT(lang)));
      expect(texts).toHaveLength(6);
      for (const text of texts) expect(text).not.toMatch(/[{}]|notice\./);
    }
  });
});

describe("fileName", () => {
  it("keeps only the last path segment", () => {
    expect(fileName("C:\\Users\\me\\Desktop\\WellSnap 2026-10-22 14.05.33.png")).toBe("WellSnap 2026-10-22 14.05.33.png");
    expect(fileName("/Users/me/Desktop/shot.png")).toBe("shot.png");
    expect(fileName("C:\\Users\\me\\Desktop\\")).toBe("Desktop");
    expect(fileName("shot.png")).toBe("shot.png");
    expect(fileName("")).toBe("");
  });

  it("shortens paths in toast details but keeps messages", () => {
    expect(toastDetail("C:\\Users\\me\\Desktop\\shot.png")).toBe("shot.png");
    expect(toastDetail("D:/Shots/2026")).toBe("2026");
    expect(toastDetail("/Users/me/Pictures/")).toBe("Pictures");
    expect(toastDetail("\\\\server\\share\\a.png")).toBe("a.png");
    expect(toastDetail("clipboard is busy, try again")).toBe("clipboard is busy, try again");
    expect(toastDetail("error: a/b failed")).toBe("error: a/b failed");
    expect(toastDetail(null)).toBeNull();
    expect(toastDetail("   ")).toBeNull();
  });
});
