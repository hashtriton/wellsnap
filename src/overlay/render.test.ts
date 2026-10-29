import { describe, expect, it } from "vitest";
import { distToSegment, inflate } from "./geometry";
import { calloutsOf } from "./marks";
import {
  arrowGeometry,
  badgeMetrics,
  bubbleLayout,
  drawAnnotation,
  layoutCallouts,
  markBadge,
  markBounds,
  markerWidth,
  tailGeometry,
  textLayout,
  wrapText,
  type Ctx2D,
  type Measure,
  type RenderEnv,
} from "./render";
import type { ArrowAnn, BlurAnn, MarkAnn, Pt, Rect, RectAnn } from "./types";

const measure: Measure = (text, font) => text.length * Number(/([\d.]+)px/.exec(font)?.[1] ?? 10) * 0.55;

/** Half-width of the head triangle at point `p` on the axis (0 outside the head). */
function headHalfWidth(g: ReturnType<typeof arrowGeometry>, p: Pt): number {
  const base = { x: (g.left.x + g.right.x) / 2, y: (g.left.y + g.right.y) / 2 };
  const headLen = Math.hypot(g.tip.x - base.x, g.tip.y - base.y);
  const fromBase = Math.hypot(p.x - base.x, p.y - base.y);
  const half = Math.hypot(g.left.x - base.x, g.left.y - base.y);
  return half * (1 - fromBase / headLen);
}

describe("arrowGeometry", () => {
  it("uses Flameshot head proportions", () => {
    const g = arrowGeometry({ x: 0, y: 0 }, { x: 300, y: 0 }, 4, 1);
    expect(g.tip).toEqual({ x: 300, y: 0 });
    expect(300 - (g.left.x + g.right.x) / 2).toBeCloseTo(18 + 16);
    expect(Math.abs(g.left.y - g.right.y)).toBeCloseTo(10 + 8);
  });

  it("ends the shaft inside the head so the round cap never pokes through", () => {
    for (const size of [2, 4, 8]) {
      for (const scale of [1, 1.25, 1.5, 2]) {
        const g = arrowGeometry({ x: 10, y: 10 }, { x: 410, y: 310 }, size, scale);
        const capEnd = (() => {
          const len = Math.hypot(400, 300);
          return { x: g.shaftEnd.x + (400 / len) * (g.width / 2), y: g.shaftEnd.y + (300 / len) * (g.width / 2) };
        })();
        expect(headHalfWidth(g, g.shaftEnd)).toBeGreaterThan(g.width / 2);
        expect(headHalfWidth(g, capEnd)).toBeGreaterThan(0);
        expect(distToSegment(g.shaftEnd, { x: 10, y: 10 }, g.tip)).toBeCloseTo(0);
      }
    }
  });

  it("shrinks the head on short arrows", () => {
    const g = arrowGeometry({ x: 0, y: 0 }, { x: 20, y: 0 }, 8, 1);
    expect(20 - (g.left.x + g.right.x) / 2).toBeCloseTo(12);
  });

  it("scales with the monitor", () => {
    const a = arrowGeometry({ x: 0, y: 0 }, { x: 500, y: 0 }, 4, 1);
    const b = arrowGeometry({ x: 0, y: 0 }, { x: 1000, y: 0 }, 4, 2);
    expect(b.width).toBe(a.width * 2);
    expect(1000 - b.left.x).toBeCloseTo((500 - a.left.x) * 2);
  });
});

describe("badges and text", () => {
  it("is a circle for one or two characters and a pill for more", () => {
    expect(badgeMetrics("7", 28, measure)).toMatchObject({ w: 28, h: 28 });
    expect(badgeMetrics("A1", 28, measure)).toMatchObject({ w: 28, h: 28 });
    expect(badgeMetrics("A12", 56, measure).w).toBeGreaterThan(56);
    expect(badgeMetrics("A1", 28, measure).outline).toBe(2);
  });

  it("sizes a badge without a letter like the lettered badge of a set", () => {
    const m = (num: number): MarkAnn => ({ kind: "mark", id: 1, color: "#FF6A1A", num, p: { x: 100, y: 100 }, tail: null, comment: "" });
    const plain = { scale: 1, letter: null, measure };
    const lettered = { scale: 1, letter: "A", measure };
    expect(markBounds(m(10), plain)).toEqual(markBounds(m(10), lettered));
    expect(markBounds(m(3), plain)).toEqual(markBounds(m(3), lettered));
    expect(markBadge(m(10), plain).label).toBe("10");
    expect(markBadge(m(10), plain).font).toBe(badgeMetrics("10", 28, measure).font);
  });

  it("marker is the size plus 14 px", () => {
    expect(markerWidth(4, 1)).toBe(18);
    expect(markerWidth(8, 1.5)).toBe(33);
  });

  it("lays out text boxes from the width preset", () => {
    const l = textLayout({ p: { x: 5, y: 6 }, text: "hello", size: 4 }, 2, measure);
    expect(l.px).toBe(44);
    expect(l.box.x).toBe(5);
    expect(l.box.w).toBeCloseTo(5 * 44 * 0.55 + l.padX * 2);
    expect(l.box.h).toBe(l.lineH + l.padY * 2);
  });
});

describe("callout pointer", () => {
  it("ends in a small filled head at the target", () => {
    const g = tailGeometry({ x: 0, y: 0 }, { x: 100, y: 0 }, 1);
    expect(g.tip).toEqual({ x: 100, y: 0 });
    expect(100 - (g.left.x + g.right.x) / 2).toBeCloseTo(13);
    expect(Math.abs(g.left.y - g.right.y)).toBeCloseTo(11);
    expect(g.shaftEnd.x).toBeLessThan(100);
  });
});

describe("wrapText", () => {
  const font = "400 20px X";

  it("wraps on words", () => {
    // 11 px per char, 111 px fits 10 chars.
    expect(wrapText("one two three four", 111, font, measure)).toEqual(["one two", "three four"]);
  });

  it("breaks words longer than a line", () => {
    expect(wrapText("abcdefghijklmnopqrstuvwxyz", 111, font, measure)).toEqual(["abcdefghij", "klmnopqrst", "uvwxyz"]);
  });

  it("collapses whitespace", () => {
    expect(wrapText("  a   b  ", 111, font, measure)).toEqual(["a b"]);
  });
});

describe("bubbleLayout", () => {
  const env = { scale: 1, letter: null, measure };
  const mark = (x: number, y: number, comment: string, tail: Pt | null = null): MarkAnn => ({
    kind: "mark",
    id: 1,
    color: "#FF6A1A",
    num: 1,
    p: { x, y },
    tail,
    comment,
  });
  const crop: Rect = { x: 0, y: 0, w: 1000, h: 800 };
  const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  const within = (r: Rect, c: Rect) => r.x >= c.x && r.y >= c.y && r.x + r.w <= c.x + c.w + 0.01 && r.y + r.h <= c.y + c.h + 0.01;

  it("has nothing to draw for an empty comment", () => {
    expect(bubbleLayout(mark(100, 100, "   "), env, crop)).toBeNull();
  });

  it("sits to the right of the badge, vertically centered", () => {
    const l = bubbleLayout(mark(100, 100, "hello"), env, crop)!;
    const badge = markBounds(mark(100, 100, "hello"), env);
    expect(l.side).toBe("right");
    expect(l.box.x).toBe(badge.x + badge.w + 6);
    expect(l.box.y + l.box.h / 2).toBeCloseTo(100);
    expect(l.lines).toEqual(["hello"]);
    expect(l.font).toContain("14px");
  });

  it("uses 14 css px times the monitor scale", () => {
    const l = bubbleLayout(mark(100, 100, "hello"), { ...env, scale: 2 }, crop)!;
    expect(l.font).toContain("28px");
  });

  it("flips left at the right edge of the crop", () => {
    const l = bubbleLayout(mark(980, 100, "hello"), env, crop)!;
    expect(l.side).toBe("left");
    expect(within(l.box, crop)).toBe(true);
  });

  it("goes below at the top and above at the bottom of a narrow crop", () => {
    const narrow: Rect = { x: 0, y: 0, w: 120, h: 400 };
    const top = bubbleLayout(mark(60, 30, "hello world"), env, narrow)!;
    expect(top.side).toBe("below");
    expect(within(top.box, narrow)).toBe(true);
    const bottom = bubbleLayout(mark(60, 380, "hello world"), env, narrow)!;
    expect(bottom.side).toBe("above");
    expect(within(bottom.box, narrow)).toBe(true);
  });

  it("slides along the badge to stay inside the top and bottom edges", () => {
    const top = bubbleLayout(mark(100, 5, "hello"), env, crop)!;
    expect(top.side).toBe("right");
    expect(top.box.y).toBe(4);
    const bottom = bubbleLayout(mark(100, 798, "hello"), env, crop)!;
    expect(bottom.side).toBe("right");
    expect(bottom.box.y + bottom.box.h).toBeCloseTo(796);
  });

  it("is placed on a crop offset from the frame origin", () => {
    const offset: Rect = { x: 500, y: 300, w: 400, h: 300 };
    const l = bubbleLayout(mark(880, 320, "hello"), env, offset)!;
    expect(l.side).toBe("left");
    expect(within(l.box, offset)).toBe(true);
  });

  it("keeps clear of the side its pointer goes to", () => {
    expect(bubbleLayout(mark(300, 300, "hello", { x: 500, y: 320 }), env, crop)!.side).toBe("left");
    expect(bubbleLayout(mark(300, 300, "hello", { x: 100, y: 300 }), env, crop)!.side).toBe("right");
  });

  it("clamps into the crop as a last resort", () => {
    // The badge is outside the crop and the crop is too low for any side as is.
    const low: Rect = { x: 0, y: 0, w: 400, h: 40 };
    const m = mark(-20, 20, "hi");
    const l = bubbleLayout(m, env, low)!;
    expect(within(l.box, low)).toBe(true);
    expect(overlaps(l.box, markBounds(m, env))).toBe(false);
  });

  it("wraps long text at about 300 css px", () => {
    const l = bubbleLayout(mark(50, 300, "очень длинный комментарий к сноске ".repeat(6)), env, crop)!;
    expect(l.lines.length).toBeGreaterThan(1);
    expect(l.box.w).toBeLessThanOrEqual(300);
    for (const line of l.lines) expect(measure(line, l.font)).toBeLessThanOrEqual(300 - 2 * l.padX + 0.01);
    expect(l.box.h).toBe(l.lines.length * l.lineH + 2 * l.padY);
  });

  it("hard-wraps a long word", () => {
    const l = bubbleLayout(mark(50, 300, "x".repeat(120)), env, crop)!;
    expect(l.lines.length).toBeGreaterThan(2);
    expect(l.lines.join("")).toBe("x".repeat(120));
    for (const line of l.lines) expect(measure(line, l.font)).toBeLessThanOrEqual(300 - 2 * l.padX + 0.01);
  });

  it("wraps to a crop narrower than the bubble width", () => {
    const small: Rect = { x: 0, y: 0, w: 200, h: 400 };
    const l = bubbleLayout(mark(20, 200, "слово ".repeat(20)), env, small)!;
    expect(l.box.w).toBeLessThanOrEqual(small.w - 8);
    expect(within(l.box, small)).toBe(true);
  });

  it("cuts text that does not fit the crop height with an ellipsis", () => {
    const short: Rect = { x: 0, y: 0, w: 600, h: 80 };
    const l = bubbleLayout(mark(20, 40, "слово ".repeat(200)), env, short)!;
    expect(l.lines[l.lines.length - 1].endsWith("…")).toBe(true);
    expect(l.box.h).toBeLessThanOrEqual(short.h - 8);
  });

  it("never covers its own badge and stays inside a roomy crop", () => {
    const crops: Rect[] = [crop, { x: 0, y: 0, w: 400, h: 300 }, { x: 0, y: 0, w: 90, h: 60 }, { x: 0, y: 0, w: 60, h: 400 }];
    const tails: (Pt | null)[] = [null, { x: 1, y: 0 }, { x: -1, y: 0 }, { x: 0, y: 1 }, { x: 0, y: -1 }];
    const comments = ["ok", "текст средней длины для сноски", "x".repeat(80), "слово ".repeat(60)];
    for (const c of crops) {
      for (let x = -30; x <= c.w + 30; x += 23) {
        for (let y = -30; y <= c.h + 30; y += 19) {
          for (const t of tails) {
            for (const text of comments) {
              const m = mark(x, y, text, t && { x: x + t.x * 100, y: y + t.y * 100 });
              const l = bubbleLayout(m, env, c)!;
              const badge = markBounds(m, env);
              expect(overlaps(l.box, badge), `${c.w}x${c.h} at ${x},${y}`).toBe(false);
              // A 10-line bubble cannot fit around a badge in the middle of 400 x 300 without covering it.
              const roomy = c.w >= 1000 || (c.w >= 400 && c.h >= 300 && text.length < 100);
              const badgeInside = within(badge, { x: c.x + 4, y: c.y + 4, w: c.w - 8, h: c.h - 8 });
              if (roomy && badgeInside) expect(within(l.box, c), `${c.w}x${c.h} at ${x},${y}`).toBe(true);
            }
          }
        }
      }
    }
  });
});

describe("layoutCallouts", () => {
  const env = { scale: 1, letter: null, measure };
  const mark = (id: number, num: number, x: number, y: number, comment: string): MarkAnn => ({
    kind: "mark",
    id,
    color: "#FF6A1A",
    num,
    p: { x, y },
    tail: null,
    comment,
  });
  const crop: Rect = { x: 0, y: 0, w: 1000, h: 800 };
  const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

  it("keeps a bubble off a neighbouring badge 40 px away", () => {
    const a = mark(1, 1, 300, 300, "first comment");
    const b = mark(2, 2, 340, 300, "second comment");
    // Alone, the first bubble would go right, over the second badge.
    expect(overlaps(bubbleLayout(a, env, crop)!.box, markBounds(b, env))).toBe(true);
    const placed = layoutCallouts([a, b], env, crop);
    expect(placed.map((p) => p.callout.id)).toEqual([1, 2]);
    expect(overlaps(placed[0].layout.box, markBounds(b, env))).toBe(false);
    expect(overlaps(placed[1].layout.box, markBounds(a, env))).toBe(false);
    expect(overlaps(placed[0].layout.box, placed[1].layout.box)).toBe(false);
  });

  it("falls back to the plain layout when every side is blocked", () => {
    const tight: Rect = { x: 0, y: 0, w: 200, h: 100 };
    const a = mark(1, 1, 30, 50, "x".repeat(40));
    const b = mark(2, 2, 170, 50, "y".repeat(40));
    const placed = layoutCallouts([a, b], env, tight);
    expect(placed).toHaveLength(2);
    for (const p of placed) expect(overlaps(p.layout.box, markBounds(p.callout, env))).toBe(false);
  });

  const rect = (id: number, num: number, r: Rect, comment: string): RectAnn => ({ kind: "rect", id, color: "#E5484D", size: 4, r, caption: { num, comment } });

  it("keeps the caption of a rectangle off the neighbouring frame it touches", () => {
    // Two table columns framed side by side, the selection close above them (a user's snapshot).
    const first = rect(1, 1, { x: 300, y: 492, w: 78, h: 333 }, "Интенсивность искривления");
    const second = rect(2, 2, { x: 378, y: 533, w: 100, h: 297 }, "Направление ствола");
    for (const top of [485, 440, 300]) {
      const crop = { x: 0, y: top, w: 580, h: 1150 - top };
      const [a, b] = layoutCallouts([first, second], env, crop).map((p) => p.layout.box);
      expect(overlaps(b, inflate(first.r, 2)), `top ${top}`).toBe(false);
      expect(overlaps(a, inflate(second.r, 2)), `top ${top}`).toBe(false);
      // Not even touching: a gap stays between the bubbles.
      expect(overlaps(inflate(a, 2.9), b), `top ${top}`).toBe(false);
    }
  });

  it("puts a moved bubble where the user left it, and the others make way", () => {
    const a = mark(1, 1, 300, 300, "first");
    // The second bubble moved by hand onto the spot the first one takes on its own, right of its badge.
    const auto = bubbleLayout(a, env, crop)!.box;
    const b: MarkAnn = { ...mark(2, 2, 700, 300, "second"), bubble: { x: auto.x - 700, y: auto.y - 300 } };
    const [pa, pb] = layoutCallouts([a, b], env, crop);
    expect(pb.layout.box).toMatchObject({ x: auto.x, y: auto.y });
    expect(overlaps(pa.layout.box, pb.layout.box)).toBe(false);
  });

  it("keeps a moved bubble inside the crop", () => {
    const m: MarkAnn = { ...mark(1, 1, 300, 300, "far"), bubble: { x: 5000, y: -5000 } };
    const [p] = layoutCallouts([m], env, crop);
    expect(p.layout.box.x + p.layout.box.w).toBeCloseTo(crop.w - 4);
    expect(p.layout.box.y).toBe(4);
  });

  it("lets a mark inside a captioned frame keep its text inside it", () => {
    const frame = rect(1, 1, { x: 100, y: 100, w: 800, h: 600 }, "frame");
    const inside = mark(2, 2, 500, 400, "inside");
    const placed = layoutCallouts([frame, inside], env, crop);
    const box = placed.find((p) => p.callout.id === 2)!.layout.box;
    expect(overlaps(box, frame.r) && box.x >= 100 && box.x + box.w <= 900).toBe(true);
  });

  it("has no bubble for a callout whose badge is outside the crop", () => {
    const placed = layoutCallouts([mark(1, 1, -20, 50, "outside"), mark(2, 2, 100, 100, "inside")], env, crop);
    expect(placed.map((p) => p.callout.id)).toEqual([2]);
  });
});

type Call = { name: string; args: unknown[] };

function recorder(): { ctx: Ctx2D; calls: Call[] } {
  const calls: Call[] = [];
  const props: Record<string, unknown> = {};
  const ctx = new Proxy(props, {
    get(target, name: string) {
      if (name in target) return target[name];
      return (...args: unknown[]) => calls.push({ name, args });
    },
    set(target, name: string, value) {
      target[name] = value;
      return true;
    },
  });
  return { ctx: ctx as unknown as Ctx2D, calls };
}

describe("blur", () => {
  const blur = (mode: BlurAnn["mode"]): BlurAnn => ({ kind: "blur", id: 1, color: "#212121", mode, r: { x: 10.25, y: 20.5, w: 30.5, h: 9.75 } });
  const env = (tile: boolean): RenderEnv => ({
    scale: 1,
    letter: null,
    measure,
    mosaic: () => (tile ? { image: {} as CanvasImageSource, cols: 6, rows: 2, cell: 6 } : null),
  });
  // Outward to whole pixels: x 10..41, y 20..31.
  const outward = [10, 20, 31, 11];

  it("fills a solid blur over whole pixels", () => {
    const { ctx, calls } = recorder();
    drawAnnotation(ctx, blur("solid"), env(true));
    expect(calls.find((c) => c.name === "fillRect")?.args).toEqual(outward);
  });

  it("clips the mosaic to whole pixels", () => {
    const { ctx, calls } = recorder();
    drawAnnotation(ctx, blur("mosaic"), env(true));
    expect(calls.find((c) => c.name === "rect")?.args).toEqual(outward);
    expect(calls.find((c) => c.name === "drawImage")?.args.slice(1, 3)).toEqual([10, 20]);
  });

  it("covers whole pixels with the placeholder too", () => {
    const { ctx, calls } = recorder();
    drawAnnotation(ctx, blur("mosaic"), env(false));
    expect(calls.find((c) => c.name === "rect")?.args).toEqual(outward);
    expect(calls.find((c) => c.name === "fillRect")?.args).toEqual(outward);
  });
});

describe("captions", () => {
  const env = { scale: 1, letter: null, measure };
  const drawEnv: RenderEnv = { ...env, mosaic: () => null };
  const crop: Rect = { x: 0, y: 0, w: 1000, h: 800 };
  const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  const arrow = (id: number, num: number, a: Pt, b: Pt, comment: string): ArrowAnn => ({
    kind: "arrow",
    id,
    color: "#E53935",
    size: 4,
    a,
    b,
    caption: { num, comment },
  });
  const frame = (id: number, num: number, r: Rect, comment: string): RectAnn => ({ kind: "rect", id, color: "#1E88E5", size: 4, r, caption: { num, comment } });

  it("keeps an arrow's bubble off the side its tip points to", () => {
    const cases: [Pt, string][] = [
      [{ x: 600, y: 310 }, "right"],
      [{ x: 100, y: 290 }, "left"],
      [{ x: 310, y: 600 }, "below"],
      [{ x: 290, y: 50 }, "above"],
    ];
    for (const [tip, side] of cases) {
      const [placed] = layoutCallouts([arrow(1, 1, { x: 300, y: 300 }, tip, "text")], env, crop);
      expect(placed.layout.side, side).not.toBe(side);
    }
  });

  it("keeps a rectangle's bubble off its frame", () => {
    const f = frame(1, 1, { x: 300, y: 300, w: 300, h: 200 }, "text of the frame");
    const [p] = layoutCallouts([f], env, crop);
    expect(p.layout.side).toBe("above");
    expect(overlaps(p.layout.box, inflate(f.r, 2))).toBe(false);
    // At the top edge of the crop there is no room above: left of the corner, still off the frame.
    const top = frame(1, 1, { x: 300, y: 10, w: 300, h: 200 }, "text of the frame");
    const [q] = layoutCallouts([top], env, crop);
    expect(q.layout.side).toBe("left");
    expect(overlaps(q.layout.box, inflate(top.r, 2))).toBe(false);
  });

  it("does not push a mark's bubble out of a captioned frame around it", () => {
    const f = frame(1, 1, { x: 100, y: 100, w: 600, h: 400 }, "frame");
    const m: MarkAnn = { kind: "mark", id: 2, color: "#FF6A1A", num: 2, p: { x: 300, y: 300 }, tail: null, comment: "inside" };
    const placed = layoutCallouts([f, m], env, crop);
    expect(placed.find((p) => p.callout.id === 2)!.layout.side).toBe("right");
  });

  it("keeps the bubble of a callout outside a captioned frame off that frame", () => {
    const f = frame(1, 1, { x: 300, y: 300, w: 300, h: 200 }, "frame");
    const m: MarkAnn = { kind: "mark", id: 2, color: "#FF6A1A", num: 2, p: { x: 270, y: 400 }, tail: null, comment: "outside" };
    const own = layoutCallouts([m], env, crop)[0];
    expect(own.layout.side).toBe("right");
    expect(overlaps(own.layout.box, f.r)).toBe(true);
    const next = layoutCallouts([f, m], env, crop).find((p) => p.callout.id === 2)!;
    expect(next.layout.side).toBe("left");
    expect(overlaps(next.layout.box, inflate(f.r, 2))).toBe(false);
  });

  it("lays out marks and captions together without covering a badge", () => {
    const list = [
      { kind: "mark", id: 1, color: "#FF6A1A", num: 1, p: { x: 300, y: 300 }, tail: null, comment: "first comment" } as MarkAnn,
      arrow(2, 2, { x: 340, y: 300 }, { x: 340, y: 500 }, "second comment"),
      frame(3, 3, { x: 380, y: 260, w: 200, h: 100 }, "third comment"),
      arrow(4, 4, { x: 300, y: 340 }, { x: 100, y: 340 }, "fourth comment"),
    ];
    const placed = layoutCallouts(list, env, crop);
    expect(placed.map((p) => p.callout.id)).toEqual([1, 2, 3, 4]);
    const badges = calloutsOf(list).map((c) => markBounds(c, env));
    for (const p of placed) for (const b of badges) expect(overlaps(p.layout.box, b), `bubble ${p.callout.id}`).toBe(false);
    for (const p of placed) for (const q of placed) if (p !== q) expect(overlaps(p.layout.box, q.layout.box)).toBe(false);
  });

  it("draws the rectangle's badge after its frame", () => {
    const { ctx, calls } = recorder();
    drawAnnotation(ctx, frame(1, 7, { x: 10, y: 10, w: 100, h: 50 }, ""), drawEnv);
    const names = calls.map((c) => c.name);
    const label = calls.findIndex((c) => c.name === "fillText" && c.args[0] === "7");
    expect(names.indexOf("strokeRect")).toBeGreaterThanOrEqual(0);
    expect(label).toBeGreaterThan(names.indexOf("strokeRect"));
  });

  it("draws an arrow's badge between the shaft and the head", () => {
    const { ctx, calls } = recorder();
    drawAnnotation(ctx, arrow(1, 3, { x: 10, y: 10 }, { x: 60, y: 10 }, ""), drawEnv);
    const names = calls.map((c) => c.name);
    const label = calls.findIndex((c) => c.name === "fillText" && c.args[0] === "3");
    expect(label).toBeGreaterThan(names.indexOf("stroke"));
    expect(label).toBeLessThan(names.lastIndexOf("closePath"));
  });

  it("draws no badge on a plain shape", () => {
    const { ctx, calls } = recorder();
    const { caption: _, ...plain } = arrow(1, 3, { x: 10, y: 10 }, { x: 60, y: 10 }, "");
    drawAnnotation(ctx, plain, drawEnv);
    expect(calls.some((c) => c.name === "fillText")).toBe(false);
  });
});
