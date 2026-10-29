import { describe, expect, it } from "vitest";
import {
  hitHandle,
  moveRect,
  normalize,
  placeAround,
  placeLoupe,
  placeToolbar,
  resizeRect,
  snap45,
  snapAxis,
  squareEnd,
  visibleHandles,
} from "./geometry";

const screen = { w: 1920, h: 1080 };
const horiz = { w: 600, h: 40 };
const vert = { w: 40, h: 520 };

describe("normalize", () => {
  it("orders corners from any drag direction", () => {
    expect(normalize({ x: 50, y: 40 }, { x: 10, y: 90 })).toEqual({ x: 10, y: 40, w: 40, h: 50 });
    expect(normalize({ x: 5, y: 5 }, { x: 5, y: 5 })).toEqual({ x: 5, y: 5, w: 0, h: 0 });
  });
});

describe("handles", () => {
  const r = { x: 100, y: 100, w: 200, h: 100 };

  it("hits the nearest handle within tolerance", () => {
    expect(hitHandle(r, { x: 103, y: 98 }, 8)).toBe("nw");
    expect(hitHandle(r, { x: 200, y: 106 }, 8)).toBe("n");
    expect(hitHandle(r, { x: 300, y: 200 }, 8)).toBe("se");
    expect(hitHandle(r, { x: 292, y: 150 }, 8)).toBe("e");
    expect(hitHandle(r, { x: 200, y: 150 }, 8)).toBeNull();
    expect(hitHandle(r, { x: 120, y: 100 }, 8)).toBeNull();
  });

  it("drops side handles on a small rectangle", () => {
    const small = { x: 0, y: 0, w: 12, h: 60 };
    expect(visibleHandles(small, 8)).not.toContain("n");
    expect(visibleHandles(small, 8)).toContain("e");
    expect(hitHandle(small, { x: 6, y: 0 }, 8)).toMatch(/^n[ew]$/);
  });
});

describe("resizeRect and moveRect", () => {
  const r = { x: 100, y: 100, w: 200, h: 100 };

  it("moves only the edges of the handle", () => {
    expect(resizeRect(r, "e", 50, 30)).toEqual({ x: 100, y: 100, w: 250, h: 100 });
    expect(resizeRect(r, "nw", -10, -20)).toEqual({ x: 90, y: 80, w: 210, h: 120 });
  });

  it("flips when dragged past the opposite edge", () => {
    expect(resizeRect(r, "w", 260, 0)).toEqual({ x: 300, y: 100, w: 60, h: 100 });
  });

  it("clamps to the monitor", () => {
    expect(resizeRect(r, "se", 5000, 5000, screen)).toEqual({ x: 100, y: 100, w: 1820, h: 980 });
    expect(moveRect(r, -500, 5000, screen)).toEqual({ x: 0, y: 980, w: 200, h: 100 });
  });
});

describe("snapping", () => {
  it("snaps to 45 degrees keeping the length", () => {
    const p = snap45({ x: 0, y: 0 }, { x: 100, y: 10 });
    expect(p.x).toBeCloseTo(Math.hypot(100, 10));
    expect(p.y).toBeCloseTo(0);
    const d = snap45({ x: 0, y: 0 }, { x: 50, y: 60 });
    expect(d.x).toBeCloseTo(d.y);
  });

  it("snaps to an axis and builds squares", () => {
    expect(snapAxis({ x: 0, y: 0 }, { x: 10, y: 80 })).toEqual({ x: 0, y: 80 });
    expect(squareEnd({ x: 10, y: 10 }, { x: -20, y: 15 })).toEqual({ x: -20, y: 40 });
  });
});

describe("placeToolbar", () => {
  // The primary button at the right end of the bar: the bar right-aligned with a selection dragged to its bottom-right.
  const legacy = { corner: { right: true, bottom: true }, h: horiz.w, v: vert.h };
  const copyAt = (right: boolean, bottom: boolean) => ({ corner: { right, bottom }, h: 520, v: 450 });

  it("goes below the selection, right-aligned", () => {
    const p = placeToolbar({ x: 300, y: 200, w: 800, h: 400 }, horiz, vert, screen, legacy);
    expect(p).toEqual({ x: 500, y: 608, orientation: "horizontal", side: "below" });
  });

  it("shifts inward when the selection is narrower than the bar near the left edge", () => {
    const p = placeToolbar({ x: 20, y: 200, w: 100, h: 100 }, horiz, vert, screen, legacy);
    expect(p.side).toBe("below");
    expect(p.x).toBe(8);
  });

  it("goes above when there is no room below", () => {
    const p = placeToolbar({ x: 300, y: 500, w: 800, h: 560 }, horiz, vert, screen, legacy);
    expect(p).toEqual({ x: 500, y: 452, orientation: "horizontal", side: "above" });
  });

  it("goes to the right side vertically when top and bottom are blocked", () => {
    const p = placeToolbar({ x: 300, y: 20, w: 800, h: 1040 }, horiz, vert, screen, legacy);
    expect(p.side).toBe("right");
    expect(p.orientation).toBe("vertical");
    expect(p.x).toBe(1108);
    // Bottom-aligned with the selection (1060 - 520).
    expect(p.y).toBe(540);
  });

  it("goes to the left side when the right is blocked too", () => {
    const p = placeToolbar({ x: 300, y: 20, w: 1600, h: 1040 }, horiz, vert, screen, legacy);
    expect(p).toMatchObject({ side: "left", orientation: "vertical", x: 252 });
  });

  it("goes inside the bottom-right corner for a full-screen selection", () => {
    const p = placeToolbar({ x: 0, y: 0, w: 1920, h: 1080 }, horiz, vert, screen, legacy);
    expect(p).toEqual({ x: 1920 - 8 - 600, y: 1080 - 8 - 40, orientation: "horizontal", side: "inside" });
  });

  it("skips the vertical sides when the vertical bar is taller than the monitor", () => {
    const p = placeToolbar({ x: 300, y: 20, w: 800, h: 1040 }, horiz, { w: 40, h: 1100 }, screen, legacy);
    expect(p.side).toBe("inside");
  });

  it("puts the primary button right under the end of a drag to the bottom-right", () => {
    const p = placeToolbar({ x: 300, y: 200, w: 800, h: 400 }, horiz, vert, screen, copyAt(true, true));
    expect(p).toEqual({ x: 1100 - 520, y: 608, orientation: "horizontal", side: "below" });
  });

  it("goes above with the button at the left edge after a drag to the top-left", () => {
    const p = placeToolbar({ x: 600, y: 300, w: 400, h: 200 }, horiz, vert, screen, copyAt(false, false));
    expect(p).toEqual({ x: 600 - 520, y: 300 - 8 - 40, orientation: "horizontal", side: "above" });
  });

  it("takes the other edge when the corner's edge has no room", () => {
    const p = placeToolbar({ x: 600, y: 20, w: 400, h: 200 }, horiz, vert, screen, copyAt(false, false));
    expect(p).toEqual({ x: 80, y: 228, orientation: "horizontal", side: "below" });
  });

  it("tries the corner's side first for the vertical bar", () => {
    const p = placeToolbar({ x: 300, y: 20, w: 800, h: 1040 }, horiz, vert, screen, copyAt(false, true));
    // The button would be at 1060 - 450; the bar is clamped into the monitor.
    expect(p).toEqual({ x: 300 - 8 - 40, y: 1080 - 8 - 520, orientation: "vertical", side: "left" });
  });

  it("goes inside the corner for a full-screen selection", () => {
    const p = placeToolbar({ x: 0, y: 0, w: 1920, h: 1080 }, horiz, vert, screen, copyAt(false, false));
    expect(p).toEqual({ x: 8, y: 8, orientation: "horizontal", side: "inside" });
  });

  it("never leaves the monitor", () => {
    const sels = [
      { x: 0, y: 0, w: 10, h: 10 },
      { x: 1910, y: 1070, w: 10, h: 10 },
      { x: 1900, y: 0, w: 20, h: 1080 },
      { x: 0, y: 1040, w: 1920, h: 40 },
      { x: 960, y: 0, w: 5, h: 1080 },
    ];
    const focuses = [legacy, copyAt(true, true), copyAt(true, false), copyAt(false, true), copyAt(false, false)];
    for (const sel of sels) {
      for (const focus of focuses) {
        const p = placeToolbar(sel, horiz, vert, screen, focus);
        const size = p.orientation === "horizontal" ? horiz : vert;
        expect(p.x).toBeGreaterThanOrEqual(8);
        expect(p.y).toBeGreaterThanOrEqual(8);
        expect(p.x + size.w).toBeLessThanOrEqual(1920 - 8);
        expect(p.y + size.h).toBeLessThanOrEqual(1080 - 8);
      }
    }
  });
});

describe("placeAround", () => {
  const box = { w: 280, h: 38 };

  it("uses the first side that fits", () => {
    expect(placeAround({ x: 100, y: 100, w: 28, h: 28 }, box, screen, ["right", "left"])).toMatchObject({ side: "right", x: 136 });
  });

  it("flips at the screen edge", () => {
    expect(placeAround({ x: 1800, y: 100, w: 28, h: 28 }, box, screen, ["right", "left"])).toMatchObject({ side: "left", x: 1800 - 8 - 280 });
  });

  it("clamps the cross axis", () => {
    expect(placeAround({ x: 100, y: 0, w: 28, h: 28 }, box, screen, ["right"]).y).toBe(8);
  });
});

describe("placeLoupe", () => {
  it("keeps away from the cursor and the edges", () => {
    expect(placeLoupe({ x: 100, y: 100 }, { w: 150, h: 180 }, screen, 24)).toEqual({ x: 124, y: 124 });
    expect(placeLoupe({ x: 1900, y: 1060 }, { w: 150, h: 180 }, screen, 24)).toEqual({ x: 1726, y: 856 });
  });
});
