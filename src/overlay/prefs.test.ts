import { describe, expect, it } from "vitest";
import { defaultPrefs, MARK_COLOR, parsePrefs, serializePrefs, stepWidth } from "./prefs";

describe("tool prefs", () => {
  it("defaults to red, yellow marker, orange marks, mosaic blur", () => {
    const p = defaultPrefs();
    expect(p.tools.arrow).toEqual({ color: "#E53935", size: 4 });
    expect(p.tools.marker.color).toBe("#FDD835");
    expect(p.tools.mark.color).toBe(MARK_COLOR);
    expect(p.blurMode).toBe("mosaic");
    expect(p.textPlate).toBe(false);
  });

  it("round-trips through settings", () => {
    const p = defaultPrefs();
    p.tools.rect = { color: "#1E88E5", size: 8 };
    p.blurMode = "solid";
    p.textPlate = true;
    expect(parsePrefs(JSON.parse(JSON.stringify(serializePrefs(p))))).toEqual(p);
  });

  it("ignores invalid values from a hand-edited file", () => {
    const p = parsePrefs({ arrow: { color: "red", size: 5 }, pencil: "x", blurMode: "gauss", textPlate: 1, rect: { color: "#abcdef" } });
    expect(p.tools.arrow).toEqual({ color: "#E53935", size: 4 });
    expect(p.tools.rect.color).toBe("#ABCDEF");
    expect(p.blurMode).toBe("mosaic");
    expect(p.textPlate).toBe(false);
    expect(parsePrefs(null)).toEqual(defaultPrefs());
  });

  it("keeps captions after drawing off by default, per tool", () => {
    expect(defaultPrefs().captions).toEqual({ arrow: false, rect: false });
    // Settings saved before the switch existed.
    expect(parsePrefs({ arrow: { color: "#1E88E5", size: 2 }, blurMode: "solid" }).captions).toEqual({ arrow: false, rect: false });
  });

  it("accepts only booleans for the caption switches", () => {
    expect(parsePrefs({ captions: { arrow: true, rect: "yes" } }).captions).toEqual({ arrow: true, rect: false });
    expect(parsePrefs({ captions: { arrow: 1, rect: null, pencil: true } }).captions).toEqual({ arrow: false, rect: false });
    expect(parsePrefs({ captions: "on" }).captions).toEqual({ arrow: false, rect: false });
  });

  it("round-trips the caption switches", () => {
    const p = defaultPrefs();
    p.captions.rect = true;
    const saved = serializePrefs(p);
    expect(saved.captions).toEqual({ arrow: false, rect: true });
    expect(parsePrefs(JSON.parse(JSON.stringify(saved))).captions).toEqual({ arrow: false, rect: true });
  });

  it("steps through S/M/L and clamps", () => {
    expect(stepWidth(2, 1)).toBe(4);
    expect(stepWidth(4, 1)).toBe(8);
    expect(stepWidth(8, 1)).toBe(8);
    expect(stepWidth(2, -1)).toBe(2);
    expect(stepWidth(3, -1)).toBe(2);
  });
});
