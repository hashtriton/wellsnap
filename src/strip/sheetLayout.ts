// Layout of a one-picture sheet: the snapshots of a set in a grid, in reading order, each headed "Снимок A"
// with its text comments under it. Pure, so the sizes are testable; composer.ts draws the result.
// All sizes are output pixels.
import { UI_FONT, wrapText, type Measure } from "../overlay/render";
import type { Snapshot } from "../shared/ipc";

/** Soft cap: Claude's high-resolution tier (2576 px, 4784 visual tokens of 28x28 px) and Telegram's 2560 px. */
export const SOFT_SIDE = 2560;
export const SOFT_TOKENS = 4784;
const TOKEN_PATCH = 28;
/** Hard cap: WhatsApp HD long side, and 12 MP keeps the clipboard bitmap under 48 MB. Same in sheet.rs. */
export const HARD_SIDE = 4096;
export const HARD_AREA = 12_000_000;
/** Output pixels per CSS pixel of the screenshots: below this their UI text gets too small to read. */
export const E_MIN = 0.75;
const MAX_COLS = 6;
const SEARCH_STEPS = 12;
/** More columns win only with a factor this much larger: fewer columns read better on phones. */
const COLUMN_TIE = 1.02;
/** Comment column of a small crop, in body sizes, so a comment does not wrap after every word. */
const MIN_TEXT_WIDTH = 16;
/** Wider comment columns, in body sizes, for a set whose comments do not fit the narrow ones: below the
 * legibility floor the body font stops shrinking at 16 px, so only a wider column can make the text shorter. */
const WIDE_TEXT_WIDTHS = [32, 64, 128];

export const SHEET_BACKGROUND = "#FFFFFF";
export const SHEET_BORDER = "#DFE3EA";
export const SHEET_TEXT = "#15181E";

/** One line of text; `y` is the top of its line box of `height`. */
export interface SheetText {
  text: string;
  x: number;
  y: number;
  height: number;
  font: string;
}

export interface SheetCell {
  letter: string;
  heading: SheetText;
  /** Where the snapshot goes, already scaled. */
  image: { x: number; y: number; w: number; h: number };
  /** Comment labels ("A1") and the wrapped comment text. */
  lines: SheetText[];
}

export interface SheetPlan {
  width: number;
  height: number;
  /** One factor for every snapshot (after the mixed-DPI normalisation); never above 1. */
  f: number;
  cols: number;
  /** The text in the screenshots ends up smaller than E_MIN: an album would read better. */
  cramped: boolean;
  cells: SheetCell[];
}

export interface SheetOptions {
  /** Heading word, "Снимок" / "Screenshot". */
  title: string;
  /** Draw the text comments under each snapshot. */
  withComments: boolean;
  measure: Measure;
}

interface Item {
  letter: string;
  baseW: number;
  baseH: number;
  heading: string;
  comments: { label: string; text: string }[];
}

interface Sizes {
  body: number;
  headFont: string;
  labelFont: string;
  bodyFont: string;
  headLine: number;
  bodyLine: number;
  pad: number;
}

function sizes(sMin: number, f: number): Sizes {
  const body = Math.min(28, Math.max(16, Math.round(16 * sMin * f)));
  const head = Math.round(1.3 * body);
  return {
    body,
    headFont: `700 ${head}px ${UI_FONT}`,
    labelFont: `700 ${body}px ${UI_FONT}`,
    bodyFont: `${body}px ${UI_FONT}`,
    headLine: Math.round(1.35 * head),
    bodyLine: Math.round(1.35 * body),
    pad: Math.round(1.5 * body),
  };
}

function saneScale(scale: number): number {
  return Number.isFinite(scale) && scale > 0 ? scale : 1;
}

/** Positions inside the cell (top left at 0, 0) and the cell's size. */
function layoutCell(item: Item, f: number, s: Sizes, textWidth: number, measure: Measure) {
  const w = Math.max(1, Math.round(item.baseW * f));
  const h = Math.max(1, Math.round(item.baseH * f));
  const hasText = item.comments.length > 0;
  const width = Math.ceil(Math.max(w, measure(item.heading, s.headFont), hasText ? textWidth * s.body : 0));
  const imageY = s.headLine + Math.round(0.5 * s.body);
  const lines: SheetText[] = [];
  let y = imageY + h + Math.round(0.6 * s.body);
  for (const c of item.comments) {
    const indent = Math.round(measure(c.label, s.labelFont) + 0.4 * s.body);
    lines.push({ text: c.label, x: 0, y, height: s.bodyLine, font: s.labelFont });
    for (const text of wrapText(c.text, width - indent, s.bodyFont, measure)) {
      lines.push({ text, x: indent, y, height: s.bodyLine, font: s.bodyFont });
      y += s.bodyLine;
    }
  }
  const height = hasText ? y : imageY + h;
  return { width, height, image: { x: 0, y: imageY, w, h }, lines };
}

function layout(items: Item[], cols: number, f: number, sMin: number, textWidth: number, measure: Measure): Omit<SheetPlan, "cramped"> {
  const s = sizes(sMin, f);
  const cells = items.map((item) => layoutCell(item, f, s, textWidth, measure));
  const rows = Math.ceil(items.length / cols);
  const colW = Array.from({ length: cols }, (_, c) => Math.max(0, ...cells.filter((_, i) => i % cols === c).map((x) => x.width)));
  const rowH = Array.from({ length: rows }, (_, r) => Math.max(0, ...cells.slice(r * cols, (r + 1) * cols).map((x) => x.height)));
  const colX = colW.map((_, c) => s.pad + colW.slice(0, c).reduce((a, b) => a + b, 0) + c * s.pad);
  const rowY = rowH.map((_, r) => s.pad + rowH.slice(0, r).reduce((a, b) => a + b, 0) + r * s.pad);
  const width = 2 * s.pad + colW.reduce((a, b) => a + b, 0) + (cols - 1) * s.pad;
  const height = 2 * s.pad + rowH.reduce((a, b) => a + b, 0) + (rows - 1) * s.pad;
  return {
    width,
    height,
    f,
    cols,
    cells: cells.map((cell, i) => {
      const x = colX[i % cols];
      const y = rowY[Math.floor(i / cols)];
      return {
        letter: items[i].letter,
        heading: { text: items[i].heading, x, y, height: s.headLine, font: s.headFont },
        image: { ...cell.image, x: x + cell.image.x, y: y + cell.image.y },
        lines: cell.lines.map((l) => ({ ...l, x: x + l.x, y: y + l.y })),
      };
    }),
  };
}

type Box = Pick<SheetPlan, "width" | "height">;

export function fitsSoft(p: Box): boolean {
  return Math.max(p.width, p.height) <= SOFT_SIDE && Math.ceil(p.width / TOKEN_PATCH) * Math.ceil(p.height / TOKEN_PATCH) <= SOFT_TOKENS;
}

export function fitsHard(p: Box): boolean {
  return Math.max(p.width, p.height) <= HARD_SIDE && p.width * p.height <= HARD_AREA;
}

/**
 * Plans the sheet: for each column count the largest factor under the soft cap; when that makes the
 * screenshots' text smaller than E_MIN, the factor that keeps E_MIN under the hard cap (smallest long side);
 * when even that does not fit, the largest factor under the hard cap, marked cramped. The last two may widen
 * the comment column; only text that does not fit even the widest one throws.
 */
export function planSheet(snapshots: readonly Snapshot[], opts: SheetOptions): SheetPlan {
  if (snapshots.length === 0) throw new Error("no snapshots to compose");
  const sMin = Math.min(...snapshots.map((s) => saneScale(s.scale)));
  const items: Item[] = snapshots.map((s) => {
    // A 2x screenshot next to a 1x one shows its UI text at the same size.
    const k = sMin / saneScale(s.scale);
    const comments =
      opts.withComments && !s.commentsOnImage
        ? s.marks.filter((m) => m.comment.trim() !== "").map((m) => ({ label: `${s.letter}${m.num}`, text: m.comment }))
        : [];
    return { letter: s.letter, baseW: s.width * k, baseH: s.height * k, heading: `${opts.title} ${s.letter}`, comments };
  });
  const at = (cols: number, f: number, textWidth = MIN_TEXT_WIDTH) => layout(items, cols, f, sMin, textWidth, opts.measure);
  const colCounts = Array.from({ length: Math.min(items.length, MAX_COLS) }, (_, i) => i + 1);
  // Narrow first, so a wider column wins only by fitting where a narrow one does not, or a larger factor.
  const textWidths = items.some((item) => item.comments.length > 0) ? [MIN_TEXT_WIDTH, ...WIDE_TEXT_WIDTHS] : [MIN_TEXT_WIDTH];

  /** Largest f in (0, 1] that fits; 0 when none does. */
  const largest = (cols: number, fits: (p: Box) => boolean, textWidth: number): number => {
    if (fits(at(cols, 1, textWidth))) return 1;
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < SEARCH_STEPS; i++) {
      const mid = (lo + hi) / 2;
      if (fits(at(cols, mid, textWidth))) lo = mid;
      else hi = mid;
    }
    return lo;
  };
  const best = (fits: (p: Box) => boolean, widths: number[]) => {
    let found = { cols: 1, f: 0, textWidth: MIN_TEXT_WIDTH };
    for (const textWidth of widths) {
      for (const cols of colCounts) {
        const f = largest(cols, fits, textWidth);
        if (f > found.f * COLUMN_TIE) found = { cols, f, textWidth };
      }
    }
    return found;
  };

  const soft = best(fitsSoft, [MIN_TEXT_WIDTH]);
  if (soft.f > 0 && sMin * soft.f >= E_MIN) return { ...at(soft.cols, soft.f), cramped: false };

  const target = Math.min(1, E_MIN / sMin);
  let legible: Omit<SheetPlan, "cramped"> | null = null;
  for (const textWidth of textWidths) {
    for (const cols of colCounts) {
      const p = at(cols, target, textWidth);
      if (fitsHard(p) && (!legible || Math.max(p.width, p.height) < Math.max(legible.width, legible.height))) legible = p;
    }
  }
  if (legible) return { ...legible, cramped: false };

  const hard = best(fitsHard, textWidths);
  if (hard.f === 0) throw new Error("too much text for one picture");
  return { ...at(hard.cols, hard.f, hard.textWidth), cramped: true };
}
