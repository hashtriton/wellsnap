import { describe, expect, it } from "vitest";
import { createT } from "../shared/i18n";
import { en } from "../locales/en";
import { ru } from "../locales/ru";
import { parseTab, TABS } from "./tabs";

describe("parseTab", () => {
  it("reads hashes and event payloads", () => {
    expect(parseTab("#support")).toBe("support");
    expect(parseTab("about")).toBe("about");
    expect(parseTab("#Hotkeys")).toBe("hotkeys");
    expect(parseTab("")).toBeNull();
    expect(parseTab(null)).toBeNull();
    expect(parseTab("#donate")).toBeNull();
  });

  it("has a label for every tab in both languages", () => {
    for (const lang of ["ru", "en"] as const) {
      const t = createT(lang);
      for (const tab of TABS) expect(t(`settings.nav.${tab}`)).not.toBe(`settings.nav.${tab}`);
    }
  });
});

const DASHES = new RegExp(`[${String.fromCharCode(0x2012)}-${String.fromCharCode(0x2015)}]`);

/** Every string of the windows owned here: no long or en dashes, no empty values (owner's rule). */
function strings(node: unknown, path: string, out: [string, string][]): [string, string][] {
  if (typeof node === "string") out.push([path, node]);
  else if (node && typeof node === "object") for (const [k, v] of Object.entries(node)) strings(v, `${path}.${k}`, out);
  return out;
}

describe("window strings", () => {
  for (const [lang, dict] of [
    ["ru", ru],
    ["en", en],
  ] as const) {
    it(`are clean in ${lang}`, () => {
      const all = (["strip", "notice", "settings", "onboarding", "overlay"] as const).flatMap((area) => strings(dict[area], area, []));
      expect(all.length).toBeGreaterThan(100);
      for (const [path, text] of all) {
        expect(text, path).not.toMatch(DASHES);
        expect(text.trim(), path).not.toBe("");
      }
    });
  }
});
