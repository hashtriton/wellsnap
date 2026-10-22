import { describe, expect, it } from "vitest";
import type { Measure } from "../overlay/render";
import type { Mark, Snapshot } from "../shared/ipc";
import { E_MIN, fitsHard, fitsSoft, planSheet, type SheetPlan } from "./sheetLayout";

const measure: Measure = (text, font) => text.length * Number(/([\d.]+)px/.exec(font)?.[1] ?? 10) * 0.55;

function snap(letter: string, width = 1920, height = 1080, extra: Partial<Snapshot> = {}): Snapshot {
  return { letter, width, height, marks: [], commentsOnImage: false, revision: 0, scale: 1, ...extra };
}

const letters = (n: number) => Array.from({ length: n }, (_, i) => String.fromCharCode(65 + i));
const fullHd = (n: number) => letters(n).map((l) => snap(l));
const plan = (snapshots: Snapshot[], withComments = true, title = "Screenshot") => planSheet(snapshots, { title, withComments, measure });
const marks = (...comments: string[]): Mark[] => comments.map((comment, i) => ({ num: i + 1, comment }));

/** Seeded, so a failure can be replayed. */
function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

describe("planSheet: full-HD sets", () => {
  it("keeps one screenshot at full size", () => {
    const p = plan(fullHd(1));
    expect(p).toMatchObject({ cols: 1, f: 1, cramped: false, width: 1968, height: 1164 });
    expect(p.cells[0].image).toEqual({ x: 24, y: 60, w: 1920, h: 1080 });
    expect(p.cells[0].heading).toMatchObject({ text: "Screenshot A", x: 24, y: 24 });
  });

  it("stacks two in one column under the soft cap", () => {
    const p = plan(fullHd(2));
    expect(p.cols).toBe(1);
    expect(p.f).toBeGreaterThan(0.85);
    expect(p.f).toBeLessThan(0.92);
    expect(p.cramped).toBe(false);
    expect(fitsSoft(p)).toBe(true);
  });

  it("goes over the soft cap to keep three legible", () => {
    const p = plan(fullHd(3));
    expect(p).toMatchObject({ cols: 1, f: E_MIN, cramped: false, width: 1488, height: 2634 });
    expect(fitsSoft(p)).toBe(false);
    expect(fitsHard(p)).toBe(true);
  });

  it("puts four in a 2x2 grid, the smaller long side", () => {
    const p = plan(fullHd(4));
    expect(p).toMatchObject({ cols: 2, f: E_MIN, cramped: false, width: 2952, height: 1764 });
    expect(p.cells.map((c) => [c.image.x, c.image.y])).toEqual([
      [24, 60],
      [1488, 60],
      [24, 930],
      [1488, 930],
    ]);
  });

  it("marks 26 screenshots as cramped and stays under the hard cap", () => {
    const p = plan(fullHd(26));
    expect(p.cramped).toBe(true);
    expect(p.f).toBeLessThan(E_MIN);
    expect(p.cols).toBeGreaterThanOrEqual(4);
    expect(fitsHard(p)).toBe(true);
    expect(p.cells.map((c) => c.letter).join("")).toBe(letters(26).join(""));
  });
});

describe("planSheet: text", () => {
  it("heads each snapshot in the request's language", () => {
    expect(plan(fullHd(2), true, "Снимок").cells.map((c) => c.heading.text)).toEqual(["Снимок A", "Снимок B"]);
  });

  it("writes labelled comments under the image, skipping empty ones", () => {
    const p = plan([snap("A", 1920, 1080, { marks: marks("Кнопка не нажимается", "", "  ", "Цена") })]);
    const cell = p.cells[0];
    expect(cell.lines.map((l) => l.text)).toEqual(["A1", "Кнопка не нажимается", "A4", "Цена"]);
    const imageBottom = cell.image.y + cell.image.h;
    expect(cell.lines.every((l) => l.y > imageBottom)).toBe(true);
    // The label and its text share a line; the text hangs after the label.
    expect(cell.lines[1].y).toBe(cell.lines[0].y);
    expect(cell.lines[1].x).toBeGreaterThan(cell.lines[0].x);
    expect(p.height).toBeGreaterThan(cell.lines[3].y + cell.lines[3].height);
  });

  it("draws no comment lines without comments, or with comments on the image", () => {
    const commented = [snap("A", 1920, 1080, { marks: marks("x") }), snap("B", 800, 600, { marks: marks("y"), commentsOnImage: true })];
    expect(plan(commented, false).cells.every((c) => c.lines.length === 0)).toBe(true);
    const p = plan(commented, true);
    expect(p.cells[0].lines.length).toBe(2);
    expect(p.cells[1].lines).toEqual([]);
  });

  it("widens the comment column when a full set has too much text for narrow columns", () => {
    const comment = "Поле ввода съезжает вниз при открытой клавиатуре и закрывает кнопку отправки формы. ".repeat(5);
    const snapshots = letters(26).map((l) => snap(l, 1920, 1080, { marks: marks(comment, comment, comment) }));
    const p = plan(snapshots);
    expect(p.cramped).toBe(true);
    expect(fitsHard(p)).toBe(true);
    const textLines = p.cells[0].lines.filter((l) => !l.font.startsWith("700"));
    expect(textLines.map((l) => l.text).join(" ")).toBe([comment, comment, comment].map((c) => c.trim()).join(" "));
    for (const c of p.cells) for (const l of c.lines) expect(l.y + l.height).toBeLessThanOrEqual(p.height);
  });

  it("gives up only when the text does not fit even one wide column", () => {
    expect(() => plan([snap("A", 1920, 1080, { marks: marks("слово ".repeat(16000)) })])).toThrow("too much text for one picture");
  });

  it("gives a small crop a readable comment column and wraps long comments inside it", () => {
    const long = "Этот текст налезает на соседнюю кнопку и не помещается в одну строку при такой ширине колонки";
    const p = plan([snap("A", 100, 50, { marks: marks(long) })]);
    const cell = p.cells[0];
    expect(p.width).toBeGreaterThanOrEqual(16 * 16 + 2 * 24);
    const textLines = cell.lines.filter((l) => !l.font.startsWith("700"));
    expect(textLines.length).toBeGreaterThan(1);
    expect(textLines.map((l) => l.text).join(" ")).toBe(long);
    for (const l of textLines) expect(l.x + measure(l.text, l.font)).toBeLessThanOrEqual(p.width - 24 + 0.001);
  });
});

describe("planSheet: scale", () => {
  it("shows a 2x and a 1x screenshot of the same screen at one size", () => {
    const p = plan([snap("A", 3840, 2160, { scale: 2 }), snap("B", 1920, 1080, { scale: 1 })]);
    expect(p.cells[0].image.w).toBe(p.cells[1].image.w);
    expect(p.cells[0].image.h).toBe(p.cells[1].image.h);
  });

  it("measures legibility against the smallest scale", () => {
    const retina = plan(letters(3).map((l) => snap(l, 3840, 2160, { scale: 2 })));
    expect(retina.cramped).toBe(false);
    expect(2 * retina.f).toBeGreaterThanOrEqual(E_MIN - 1e-9);
  });

  it("treats a missing or broken scale as 1", () => {
    expect(plan([snap("A", 800, 600, { scale: 0 })]).f).toBe(1);
  });

  it("refuses an empty set", () => {
    expect(() => plan([])).toThrow();
  });
});

describe("planSheet: properties over random sets", () => {
  const check = (p: SheetPlan, snapshots: Snapshot[]) => {
    const sMin = Math.min(...snapshots.map((s) => s.scale));
    expect(p.f).toBeGreaterThan(0);
    expect(p.f).toBeLessThanOrEqual(1);
    expect(fitsHard(p)).toBe(true);
    if (!p.cramped) expect(sMin * p.f).toBeGreaterThanOrEqual(E_MIN - 1e-9);
    // Under the soft cap whenever the legibility floor did not force a larger picture.
    if (!p.cramped && sMin * p.f > E_MIN + 1e-6) expect(fitsSoft(p)).toBe(true);
    expect(p.cells.map((c) => c.letter)).toEqual(snapshots.map((s) => s.letter));
    p.cells.forEach((c, i) => {
      const prev = p.cells[i - 1];
      if (!prev) return;
      // Reading order: left to right in a row, rows top to bottom.
      if (i % p.cols === 0) expect(c.heading.y).toBeGreaterThan(prev.heading.y);
      else {
        expect(c.heading.y).toBe(prev.heading.y);
        expect(c.heading.x).toBeGreaterThan(prev.heading.x);
      }
    });
    for (const c of p.cells) {
      expect(c.image.x + c.image.w).toBeLessThanOrEqual(p.width);
      expect(c.image.y + c.image.h).toBeLessThanOrEqual(p.height);
    }
  };

  it("holds the caps, the order and the legibility rule", () => {
    const rand = random(20261022);
    for (let run = 0; run < 60; run++) {
      const n = 1 + Math.floor(rand() * 26);
      const snapshots = letters(n).map((l) =>
        snap(l, 40 + Math.floor(rand() * 3800), 40 + Math.floor(rand() * 2100), {
          scale: [1, 1.25, 1.5, 2][Math.floor(rand() * 4)],
          marks: rand() < 0.5 ? marks(...Array.from({ length: Math.floor(rand() * 4) }, () => "Короткий комментарий к снимку")) : [],
          commentsOnImage: rand() < 0.2,
        }),
      );
      const p = plan(snapshots);
      check(p, snapshots);
      expect(plan(snapshots)).toEqual(p);
    }
  });
});
