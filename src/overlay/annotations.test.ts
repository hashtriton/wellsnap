import { describe, expect, it } from "vitest";
import { annotationHandles, bounds, dragHandle, hitAnnotationHandle, hitTest, topCallout, topHit, translate } from "./annotations";
import type { Measure } from "./render";
import type { Annotation, ArrowAnn, MarkAnn, PencilAnn, RectAnn } from "./types";

const measure: Measure = (text, font) => text.length * Number(/([\d.]+)px/.exec(font)?.[1] ?? 10) * 0.55;
const env = { scale: 1, letter: "A", measure };

const arrow: ArrowAnn = { kind: "arrow", id: 1, color: "#E53935", size: 4, a: { x: 0, y: 0 }, b: { x: 100, y: 0 } };
const rect: RectAnn = { kind: "rect", id: 2, color: "#E53935", size: 4, r: { x: 100, y: 100, w: 100, h: 80 } };
const mark: MarkAnn = { kind: "mark", id: 3, color: "#FF6A1A", num: 1, p: { x: 300, y: 300 }, tail: { x: 380, y: 300 }, comment: "" };
const pencil: PencilAnn = {
  kind: "pencil",
  id: 4,
  color: "#E53935",
  size: 4,
  points: [
    { x: 0, y: 200 },
    { x: 50, y: 250 },
  ],
};

describe("hitTest", () => {
  it("hits arrows near the shaft", () => {
    expect(hitTest(arrow, { x: 50, y: 4 }, env, 4)).toBe(true);
    expect(hitTest(arrow, { x: 50, y: 20 }, env, 4)).toBe(false);
  });

  it("hits rectangles on the outline only", () => {
    expect(hitTest(rect, { x: 101, y: 140 }, env, 4)).toBe(true);
    expect(hitTest(rect, { x: 150, y: 140 }, env, 4)).toBe(false);
  });

  it("hits marks on the badge and the tail", () => {
    expect(hitTest(mark, { x: 305, y: 296 }, env, 4)).toBe(true);
    expect(hitTest(mark, { x: 360, y: 301 }, env, 4)).toBe(true);
    expect(hitTest(mark, { x: 330, y: 330 }, env, 4)).toBe(false);
  });

  it("hits pencil strokes along the path", () => {
    expect(hitTest(pencil, { x: 25, y: 226 }, env, 4)).toBe(true);
    expect(hitTest(pencil, { x: 25, y: 245 }, env, 4)).toBe(false);
  });

  it("returns the topmost annotation", () => {
    const blur: Annotation = { kind: "blur", id: 9, color: "#212121", mode: "mosaic", r: { x: 90, y: 90, w: 50, h: 50 } };
    expect(topHit([rect, blur], { x: 101, y: 120 }, env, 4)?.id).toBe(9);
    expect(topHit([rect, blur], { x: 101, y: 120 }, env, 4, ["rect"])?.id).toBe(2);
  });
});

describe("captioned shapes", () => {
  // Badge 28 px ("A1" circle), centered on the arrow start and on the rectangle's top-left corner.
  const capArrow: ArrowAnn = { ...arrow, a: { x: 0, y: 100 }, b: { x: 100, y: 100 }, caption: { num: 1, comment: "a" } };
  const capRect: RectAnn = { ...rect, caption: { num: 2, comment: "" } };

  it("are hit on the badge", () => {
    expect(hitTest(capArrow, { x: -12, y: 88 }, env, 4)).toBe(true);
    expect(hitTest({ ...capArrow, caption: undefined }, { x: -12, y: 88 }, env, 4)).toBe(false);
    // Inside the frame near the corner: plain rectangles are hit on the outline only.
    expect(hitTest(capRect, { x: 112, y: 112 }, env, 4)).toBe(true);
    expect(hitTest(rect, { x: 112, y: 112 }, env, 4)).toBe(false);
  });

  it("include the badge in their bounds", () => {
    expect(bounds(capArrow, env).x).toBeLessThanOrEqual(-14);
    expect(bounds(capArrow, env).y).toBeLessThanOrEqual(86);
    expect(bounds(capRect, env)).toEqual({ x: 86, y: 86, w: 114, h: 94 });
    expect(bounds(rect, env)).toEqual(rect.r);
  });

  it("drop the top-left handle of a captioned rectangle so the badge moves it", () => {
    const ids = annotationHandles(capRect, 8).map((h) => h.id);
    expect(ids).toHaveLength(7);
    expect(ids).not.toContain("nw");
    expect(hitAnnotationHandle(capRect, { x: 100, y: 100 }, 8, env)).toBeNull();
    expect(annotationHandles(capArrow, 8).map((h) => h.id)).toEqual(["a", "b"]);
  });

  it("make the whole badge of an arrow its start handle", () => {
    // Off the 8 px square around the start but on the badge ring (radius 14, plus 2 of grab distance).
    expect(hitAnnotationHandle(capArrow, { x: -13, y: 100 }, 8, env)).toBe("a");
    expect(hitAnnotationHandle(capArrow, { x: 0, y: 115 }, 8, env)).toBe("a");
    expect(hitAnnotationHandle(capArrow, { x: 0, y: 117 }, 8, env)).toBeNull();
    expect(hitAnnotationHandle({ ...capArrow, caption: undefined }, { x: -13, y: 100 }, 8, env)).toBeNull();
    // Wherever a press on the badge would move the arrow, a press on the selected arrow edits its start.
    for (const p of [
      { x: -15, y: 100 },
      { x: 0, y: 85 },
      { x: 10, y: 110 },
    ]) {
      expect(topCallout([capArrow], p, env, 4)?.id).toBe(1);
      expect(hitAnnotationHandle(capArrow, p, 8, env)).toBe("a");
    }
  });

  it("keep the caption when moved or edited", () => {
    expect(translate(capArrow, 5, 5)).toMatchObject({ caption: { num: 1, comment: "a" }, a: { x: 5, y: 105 } });
    expect(translate(capRect, 5, 5)).toMatchObject({ caption: { num: 2, comment: "" } });
    expect(dragHandle(capRect, "se", 10, 10, false)).toMatchObject({ caption: { num: 2 } });
  });

  it("are found by their badge for the Mark tool, plain shapes are not", () => {
    const list = [rect, capArrow, mark];
    expect(topCallout(list, { x: 3, y: 97 }, env, 4)?.id).toBe(1);
    expect(topCallout(list, { x: 50, y: 100 }, env, 4)).toBeNull();
    expect(topCallout(list, { x: 100, y: 100 }, env, 4)).toBeNull();
    expect(topCallout(list, { x: 360, y: 301 }, env, 4)?.id).toBe(3);
  });
});

describe("editing", () => {
  it("moves every point", () => {
    expect(translate(mark, 10, -5)).toMatchObject({ p: { x: 310, y: 295 }, tail: { x: 390, y: 295 } });
    expect((translate(pencil, 1, 1) as PencilAnn).points[1]).toEqual({ x: 51, y: 251 });
    expect(pencil.points[1]).toEqual({ x: 50, y: 250 });
  });

  it("drags arrow ends with optional 45 degree snapping", () => {
    expect(dragHandle(arrow, "b", 0, 30, false)).toMatchObject({ b: { x: 100, y: 30 } });
    const snapped = dragHandle(arrow, "b", 0, 10, true) as ArrowAnn;
    expect(snapped.b.y).toBeCloseTo(0);
  });

  it("resizes rectangles through their handles", () => {
    expect(dragHandle(rect, "se", 20, 10, false)).toMatchObject({ r: { x: 100, y: 100, w: 120, h: 90 } });
  });

  it("exposes handles per kind", () => {
    expect(annotationHandles(arrow, 8).map((h) => h.id)).toEqual(["a", "b"]);
    expect(annotationHandles(rect, 8)).toHaveLength(8);
    expect(annotationHandles(mark, 8).map((h) => h.id)).toEqual(["tail"]);
    expect(annotationHandles(pencil, 8)).toEqual([]);
    expect(hitAnnotationHandle(arrow, { x: 97, y: 2 }, 8, env)).toBe("b");
  });
});
