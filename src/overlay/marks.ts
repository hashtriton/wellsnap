import type { Mark } from "../shared/ipc";
import { contains } from "./geometry";
import type { Annotation, Pt, Rect } from "./types";

export const MAX_MARKS = 99;

/** A numbered comment: a mark, or the caption of an arrow or a rectangle. A MarkAnn is one as is. */
export interface Callout {
  /** Id of the annotation that carries it. */
  id: number;
  color: string;
  num: number;
  comment: string;
  /** Badge center. */
  p: Pt;
  /** Point the badge points to (pointer end, arrow tip): the bubble goes to that side last. */
  tail: Pt | null;
  /** Absent for a mark. */
  host?: "arrow" | "rect";
  /** Area the bubble keeps off when it can (the stroked frame of a captioned rectangle). */
  avoid?: Rect;
  /** Bubble moved by hand: its top-left corner relative to `p`. */
  bubble?: Pt;
}

export function calloutOf(a: Annotation): Callout | null {
  switch (a.kind) {
    case "mark":
      return a;
    case "arrow":
      return a.caption
        ? { id: a.id, color: a.color, num: a.caption.num, comment: a.caption.comment, p: a.a, tail: a.b, host: "arrow", bubble: a.caption.bubble }
        : null;
    case "rect":
      return a.caption
        ? { id: a.id, color: a.color, num: a.caption.num, comment: a.caption.comment, p: { x: a.r.x, y: a.r.y }, tail: null, host: "rect", bubble: a.caption.bubble }
        : null;
    default:
      return null;
  }
}

/** The callout of `a` with its bubble at `offset` from the badge center; other annotations come back as is. */
export function withBubble(a: Annotation, offset: Pt): Annotation {
  if (a.kind === "mark") return { ...a, bubble: offset };
  if ((a.kind === "arrow" || a.kind === "rect") && a.caption) return { ...a, caption: { ...a.caption, bubble: offset } };
  return a;
}

export function calloutsOf(list: readonly Annotation[]): Callout[] {
  const out: Callout[] = [];
  for (const a of list) {
    const c = calloutOf(a);
    if (c) out.push(c);
  }
  return out;
}

/** Next number is derived from the callouts present (not a counter), so undo and delete stay consistent. */
export function nextMarkNum(list: readonly Annotation[]): number {
  let max = 0;
  for (const c of calloutsOf(list)) if (c.num > max) max = c.num;
  return max + 1;
}

/** Shifts every callout number above `num` down by one, so A1..An has no gaps. */
function closeGap(list: readonly Annotation[], num: number): Annotation[] {
  return list.map((a) => {
    if (a.kind === "mark") return a.num > num ? { ...a, num: a.num - 1 } : a;
    if ((a.kind === "arrow" || a.kind === "rect") && a.caption && a.caption.num > num) return { ...a, caption: { ...a.caption, num: a.caption.num - 1 } };
    return a;
  });
}

/** Removes an annotation; removing a callout renumbers the higher ones. */
export function removeAnnotation(list: readonly Annotation[], id: number): Annotation[] {
  const target = list.find((a) => a.id === id);
  if (!target) return [...list];
  const rest = list.filter((a) => a.id !== id);
  const c = calloutOf(target);
  return c ? closeGap(rest, c.num) : rest;
}

/**
 * Sets the caption text of the arrow or rectangle `id`: adds a caption with the next number, updates its
 * text, or (null) removes it and renumbers the higher callouts. Other annotations come back unchanged.
 */
export function setCaption(list: readonly Annotation[], id: number, comment: string | null): Annotation[] {
  const target = list.find((a) => a.id === id);
  if (!target || (target.kind !== "arrow" && target.kind !== "rect")) return [...list];
  if (comment === null) {
    if (!target.caption) return [...list];
    const { caption, ...plain } = target;
    return closeGap(
      list.map((a) => (a.id === id ? plain : a)),
      caption.num,
    );
  }
  const caption = target.caption ? { ...target.caption, comment } : { num: nextMarkNum(list), comment };
  return list.map((a) => (a.id === id ? { ...target, caption } : a));
}

export function markLabel(letter: string | null, num: number): string {
  return `${letter ?? ""}${num}`;
}

/**
 * A callout belongs to the snapshot when its badge center lies inside the crop. One rule for the bubbles on
 * screen, the exported image and the text block, so a comment never goes out without its badge.
 */
export function markInCrop(a: Pick<Callout, "p">, crop: Rect): boolean {
  return contains(crop, a.p);
}

export function calloutsInCrop(list: readonly Annotation[], crop: Rect): Callout[] {
  return calloutsOf(list).filter((c) => markInCrop(c, crop));
}

/** Callouts of the crop for the text block, ordered by number. */
export function marksMeta(list: readonly Annotation[], crop: Rect): Mark[] {
  return calloutsInCrop(list, crop)
    .sort((a, b) => a.num - b.num)
    .map((m) => ({ num: m.num, comment: m.comment.trim() }));
}

function channels(hex: string): [number, number, number] {
  const v = parseInt(hex.slice(1), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

/**
 * Black or white text for a badge fill by perceived luma. The threshold sits above the midpoint so the
 * brand orange (#FF6A1A, luma 0.55) keeps white text like the other saturated presets.
 */
export function contrastText(hex: string): string {
  const [r, g, b] = channels(hex);
  const luma = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luma > 0.6 ? "#000000" : "#FFFFFF";
}
