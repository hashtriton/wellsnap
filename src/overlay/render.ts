// One renderer for the screen and the exported image, so the result always matches what the user saw.
import { clamp, contains, inflate, type Side } from "./geometry";
import { calloutOf, contrastText, markInCrop, markLabel, type Callout } from "./marks";
import type { Annotation, ArrowAnn, BlurAnn, MarkAnn, Pt, Rect, TextAnn } from "./types";

export type Ctx2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
/** Width of `text` drawn with the CSS `font`. */
export type Measure = (text: string, font: string) => number;

// Same stack as --font-ui in theme.css; canvas cannot read CSS variables in a font string.
export const UI_FONT =
  '"Segoe UI Variable Text", "Segoe UI", -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", Arial, sans-serif';

export const MARKER_OPACITY = 0.5;
const MARKER_PAD = 14;
export const BADGE_HEIGHT = 28;
const MARK_TAIL_WIDTH = 3;
const MARK_HEAD_LEN = 13;
const MARK_HEAD_WIDTH = 11;

// Callout bubble, CSS px (multiplied by the monitor scale).
const BUBBLE_FONT = 14;
const BUBBLE_MAX_W = 300;
const BUBBLE_GAP = 6;
/** Distance kept from the edges of the crop. */
const BUBBLE_MARGIN = 4;
const BUBBLE_TEXT = "#15181E";

export interface MosaicTile {
  image: CanvasImageSource;
  cols: number;
  rows: number;
  cell: number;
}

export interface RenderEnv {
  /** Monitor scale (physical px per CSS px). */
  scale: number;
  /** Snapshot letter drawn into the badges, null for plain numbers. */
  letter: string | null;
  measure: Measure;
  /** Mosaic for a blur box; null while the frame pixels are not available yet. */
  mosaic(a: BlurAnn): MosaicTile | null;
}

export interface ArrowGeometry {
  tip: Pt;
  left: Pt;
  right: Pt;
  shaftEnd: Pt;
  width: number;
}

/**
 * Arrow with a filled head (Flameshot proportions: head length 18 + 4w, head width 10 + 2w). The shaft ends
 * inside the head, so its round cap never pokes through the sides or the tip. Short arrows get a smaller head.
 */
export function arrowGeometry(a: Pt, b: Pt, size: number, scale: number): ArrowGeometry {
  return headGeometry(a, b, size * scale, (18 + 4 * size) * scale, (10 + 2 * size) * scale);
}

function headGeometry(a: Pt, b: Pt, width: number, headLen: number, headWidth: number): ArrowGeometry {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len === 0) return { tip: b, left: b, right: b, shaftEnd: a, width };
  if (headLen > len * 0.6) {
    const k = (len * 0.6) / headLen;
    headLen *= k;
    headWidth *= k;
  }
  const ux = (b.x - a.x) / len;
  const uy = (b.y - a.y) / len;
  const base = { x: b.x - ux * headLen, y: b.y - uy * headLen };
  const half = headWidth / 2;
  return {
    tip: b,
    left: { x: base.x - uy * half, y: base.y + ux * half },
    right: { x: base.x + uy * half, y: base.y - ux * half },
    shaftEnd: { x: base.x + ux * headLen * 0.35, y: base.y + uy * headLen * 0.35 },
    width,
  };
}

export function markerWidth(size: number, scale: number): number {
  return (size + MARKER_PAD) * scale;
}

export interface BadgeMetrics {
  w: number;
  h: number;
  font: string;
  outline: number;
}

/** Circle for one or two characters, pill for longer labels ("A12"). */
export function badgeMetrics(label: string, h: number, measure: Measure): BadgeMetrics {
  const short = label.length <= 2;
  const font = `700 ${h * (short ? 0.5 : 0.465)}px ${UI_FONT}`;
  const outline = h / 14;
  if (short) return { w: h, h, font, outline };
  return { w: Math.max(h, measure(label, font) + h * 0.64), h, font, outline };
}

export function drawBadge(ctx: Ctx2D, cx: number, cy: number, label: string, color: string, m: BadgeMetrics): void {
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(cx - m.w / 2, cy - m.h / 2, m.w, m.h, m.h / 2);
  // Stroke first at double width: the fill then covers the inner half and leaves an outer white ring.
  ctx.lineWidth = m.outline * 2;
  ctx.strokeStyle = "#FFFFFF";
  ctx.stroke();
  ctx.fillStyle = color;
  ctx.fill();
  ctx.fillStyle = contrastText(color);
  ctx.font = m.font;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(label, cx, cy + m.h * 0.02);
  ctx.restore();
}

export function markBadge(a: Pick<Callout, "num">, env: Pick<RenderEnv, "scale" | "letter" | "measure">): BadgeMetrics & { label: string } {
  const label = markLabel(env.letter, a.num);
  const h = BADGE_HEIGHT * env.scale;
  // Sized for the label the badge gets in a set ("A10", not "10"): the first add to a set exports with the
  // letter, and the badge and its bubble must keep the geometry the screen showed.
  const m = badgeMetrics(markLabel(env.letter ?? "A", a.num), h, env.measure);
  return { ...m, font: badgeMetrics(label, h, env.measure).font, label };
}

export function markBounds(a: Pick<Callout, "p" | "num">, env: Pick<RenderEnv, "scale" | "letter" | "measure">): Rect {
  const m = markBadge(a, env);
  return { x: a.p.x - m.w / 2, y: a.p.y - m.h / 2, w: m.w, h: m.h };
}

export interface TextLayout {
  font: string;
  px: number;
  padX: number;
  padY: number;
  lineH: number;
  box: Rect;
}

/** Font size in CSS px for the width presets 2/4/8: 16/22/34. */
export function textPx(size: number): number {
  return 10 + 3 * size;
}

export function textLayout(a: Pick<TextAnn, "p" | "text" | "size">, scale: number, measure: Measure): TextLayout {
  const px = textPx(a.size) * scale;
  const font = `600 ${px}px ${UI_FONT}`;
  const padX = Math.round(px * 0.35);
  const padY = Math.round(px * 0.18);
  const lineH = Math.round(px * 1.3);
  const w = measure(a.text, font) + padX * 2;
  return { font, px, padX, padY, lineH, box: { x: a.p.x, y: a.p.y, w, h: lineH + padY * 2 } };
}

function strokeLine(ctx: Ctx2D, a: Pt, b: Pt): void {
  ctx.beginPath();
  ctx.moveTo(a.x, a.y);
  ctx.lineTo(b.x, b.y);
  ctx.stroke();
}

function drawCaptionBadge(ctx: Ctx2D, c: Callout, env: RenderEnv): void {
  const m = markBadge(c, env);
  drawBadge(ctx, c.p.x, c.p.y, m.label, c.color, m);
}

function drawArrow(ctx: Ctx2D, a: ArrowAnn, env: RenderEnv): void {
  const scale = env.scale;
  const g = arrowGeometry(a.a, a.b, a.size, scale);
  ctx.strokeStyle = a.color;
  ctx.fillStyle = a.color;
  ctx.lineWidth = g.width;
  ctx.lineCap = "round";
  strokeLine(ctx, a.a, g.shaftEnd);
  // The caption badge goes between the shaft and the head, so the head of a short arrow stays visible.
  const caption = calloutOf(a);
  if (caption) drawCaptionBadge(ctx, caption, env);
  ctx.beginPath();
  ctx.moveTo(g.tip.x, g.tip.y);
  ctx.lineTo(g.left.x, g.left.y);
  ctx.lineTo(g.right.x, g.right.y);
  ctx.closePath();
  ctx.lineJoin = "round";
  ctx.lineWidth = Math.max(1, scale);
  ctx.fill();
  ctx.stroke();
}

function drawPencil(ctx: Ctx2D, points: Pt[], width: number, color: string): void {
  if (points.length === 0) return;
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  if (points.length === 1) {
    ctx.beginPath();
    ctx.arc(points[0].x, points[0].y, width / 2, 0, Math.PI * 2);
    ctx.fill();
    return;
  }
  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);
  // Quadratic curves through the midpoints: light smoothing without lagging behind the pointer.
  for (let i = 1; i < points.length - 1; i++) {
    const p = points[i];
    const n = points[i + 1];
    ctx.quadraticCurveTo(p.x, p.y, (p.x + n.x) / 2, (p.y + n.y) / 2);
  }
  const last = points[points.length - 1];
  ctx.lineTo(last.x, last.y);
  ctx.stroke();
}

function drawBlur(ctx: Ctx2D, a: BlurAnn, env: RenderEnv): void {
  // Outward to whole pixels, like the protected area of the mosaic: an anti-aliased fractional edge would
  // blend the original pixels of the outermost row and column into the output.
  const x0 = Math.floor(a.r.x);
  const y0 = Math.floor(a.r.y);
  const w = Math.ceil(a.r.x + a.r.w) - x0;
  const h = Math.ceil(a.r.y + a.r.h) - y0;
  if (a.mode === "solid") {
    ctx.fillStyle = a.color;
    ctx.fillRect(x0, y0, w, h);
    return;
  }
  const tile = env.mosaic(a);
  ctx.beginPath();
  ctx.rect(x0, y0, w, h);
  ctx.clip();
  if (!tile) {
    ctx.fillStyle = "#808080";
    ctx.fillRect(x0, y0, w, h);
    return;
  }
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(tile.image, x0, y0, tile.cols * tile.cell, tile.rows * tile.cell);
}

/** Pointer of a callout: from the badge center to the target, with a small filled head at the target. */
export function tailGeometry(from: Pt, to: Pt, scale: number): ArrowGeometry {
  return headGeometry(from, to, MARK_TAIL_WIDTH * scale, MARK_HEAD_LEN * scale, MARK_HEAD_WIDTH * scale);
}

function headPath(ctx: Ctx2D, g: ArrowGeometry): void {
  ctx.beginPath();
  ctx.moveTo(g.tip.x, g.tip.y);
  ctx.lineTo(g.left.x, g.left.y);
  ctx.lineTo(g.right.x, g.right.y);
  ctx.closePath();
}

function drawMark(ctx: Ctx2D, a: MarkAnn, env: RenderEnv): void {
  const s = env.scale;
  if (a.tail) {
    const g = tailGeometry(a.p, a.tail, s);
    // White outline first, so the pointer reads on any background.
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = "#FFFFFF";
    ctx.lineWidth = g.width + 2 * s;
    strokeLine(ctx, a.p, g.shaftEnd);
    headPath(ctx, g);
    ctx.lineWidth = 2 * s;
    ctx.stroke();
    ctx.strokeStyle = a.color;
    ctx.fillStyle = a.color;
    ctx.lineWidth = g.width;
    strokeLine(ctx, a.p, g.shaftEnd);
    headPath(ctx, g);
    ctx.lineWidth = Math.max(1, s);
    ctx.fill();
    ctx.stroke();
  }
  const m = markBadge(a, env);
  drawBadge(ctx, a.p.x, a.p.y, m.label, a.color, m);
}

/** Greedy word wrap; words longer than the line are broken by characters. */
export function wrapText(text: string, maxW: number, font: string, measure: Measure): string[] {
  const lines: string[] = [];
  let line = "";
  const pushWord = (word: string) => {
    const candidate = line ? `${line} ${word}` : word;
    if (measure(candidate, font) <= maxW) {
      line = candidate;
      return;
    }
    if (line) lines.push(line);
    line = "";
    if (measure(word, font) <= maxW) {
      line = word;
      return;
    }
    let chunk = "";
    for (const ch of word) {
      if (chunk && measure(chunk + ch, font) > maxW) {
        lines.push(chunk);
        chunk = "";
      }
      chunk += ch;
    }
    line = chunk;
  };
  for (const word of text.split(/\s+/).filter(Boolean)) pushWord(word);
  if (line) lines.push(line);
  return lines;
}

/** Keeps at most `max` lines; the last kept line ends with an ellipsis when text was cut. */
function clampLines(lines: string[], max: number, maxW: number, font: string, measure: Measure): string[] {
  if (lines.length <= max) return lines;
  const kept = lines.slice(0, max);
  let last = kept[kept.length - 1];
  while (last && measure(`${last}…`, font) > maxW) last = last.slice(0, -1);
  kept[kept.length - 1] = `${last}…`;
  return kept;
}

export type CalloutStyle = "solid" | "ghost";

export interface BubbleLayout {
  box: Rect;
  side: Side;
  lines: string[];
  font: string;
  padX: number;
  padY: number;
  lineH: number;
  radius: number;
}

function intersects(a: Rect, b: Rect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

function inside(r: Rect, room: Rect): boolean {
  const e = 0.01;
  return r.x >= room.x - e && r.y >= room.y - e && r.x + r.w <= room.x + room.w + e && r.y + r.h <= room.y + room.h + e;
}

/**
 * Right first, then left, below, above; the side the pointer leaves towards goes last, so the text does not
 * cover the target. A rectangle's badge sits on its top-left corner: above and left lead away from the frame.
 */
function bubbleSides(a: Callout): Side[] {
  if (a.host === "rect") return ["above", "left", "below", "right"];
  const sides: Side[] = ["right", "left", "below", "above"];
  if (!a.tail) return sides;
  const dx = a.tail.x - a.p.x;
  const dy = a.tail.y - a.p.y;
  const towards: Side = Math.abs(dx) >= Math.abs(dy) ? (dx > 0 ? "right" : "left") : dy > 0 ? "below" : "above";
  return [...sides.filter((s) => s !== towards), towards];
}

/**
 * Bubble with the comment of a callout, next to its badge and inside `crop` (physical px). Tries the sides in
 * order, sliding along the badge to stay inside and off `obstacles` (other badges and bubbles); then the
 * first side that only stays inside; if none fits, clamps into the crop on the first side that stays off the
 * badge. Null for an empty comment.
 */
export function bubbleLayout(
  a: Callout,
  env: Pick<RenderEnv, "scale" | "letter" | "measure">,
  crop: Rect,
  obstacles: readonly Rect[] = [],
): BubbleLayout | null {
  const text = a.comment.trim();
  if (!text) return null;
  const s = env.scale;
  const px = BUBBLE_FONT * s;
  const font = `500 ${px}px ${UI_FONT}`;
  const padX = Math.round(9 * s);
  const padY = Math.round(5 * s);
  const lineH = Math.round(px * 1.35);
  const gap = BUBBLE_GAP * s;
  const margin = BUBBLE_MARGIN * s;
  const room = { x: crop.x + margin, y: crop.y + margin, w: Math.max(0, crop.w - 2 * margin), h: Math.max(0, crop.h - 2 * margin) };

  const maxTextW = Math.max(px * 3, Math.min(BUBBLE_MAX_W * s, room.w) - 2 * padX);
  const maxLines = Math.max(1, Math.floor((room.h - 2 * padY) / lineH));
  const lines = clampLines(wrapText(text, maxTextW, font, env.measure), maxLines, maxTextW, font, env.measure);
  const w = Math.ceil(Math.max(...lines.map((l) => env.measure(l, font))) + 2 * padX);
  const h = lines.length * lineH + 2 * padY;

  const badge = markBounds(a, env);
  const cx = badge.x + badge.w / 2;
  const cy = badge.y + badge.h / 2;
  const slideY = (y: number) => clamp(y, room.y, Math.max(room.y, room.y + room.h - h));
  const slideX = (x: number) => clamp(x, room.x, Math.max(room.x, room.x + room.w - w));
  const at = (side: Side): Rect => {
    switch (side) {
      case "right":
        return { x: badge.x + badge.w + gap, y: slideY(cy - h / 2), w, h };
      case "left":
        return { x: badge.x - gap - w, y: slideY(cy - h / 2), w, h };
      case "below":
        return { x: slideX(cx - w / 2), y: badge.y + badge.h + gap, w, h };
      case "above":
        return { x: slideX(cx - w / 2), y: badge.y - gap - h, w, h };
    }
  };
  const layout = (box: Rect, side: Side): BubbleLayout => ({ box, side, lines, font, padX, padY, lineH, radius: 6 * s });

  const clear = (box: Rect) => !obstacles.some((o) => intersects(box, o));
  const sides = bubbleSides(a);
  for (const side of sides) {
    const box = at(side);
    if (inside(box, room) && clear(box)) return layout(box, side);
  }
  for (const side of sides) {
    const box = at(side);
    if (inside(box, room)) return layout(box, side);
  }
  const clamped = sides.map((side) => {
    const box = at(side);
    return { side, box: { ...box, x: slideX(box.x), y: slideY(box.y) } };
  });
  const offBadge = clamped.filter((c) => !intersects(c.box, badge));
  const best = offBadge.find((c) => clear(c.box)) ?? offBadge[0];
  if (best) return layout(best.box, best.side);
  // A crop too small for any clamped position: stay next to the badge, the crop clips the rest.
  return layout(at(sides[0]), sides[0]);
}

function drawBubble(ctx: Ctx2D, l: BubbleLayout, color: string, style: CalloutStyle, scale: number): void {
  const b = l.box;
  const lw = Math.max(1, 1.5 * scale);
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(b.x, b.y, b.w, b.h, l.radius);
  if (style === "solid") {
    ctx.shadowColor = "rgba(0, 0, 0, 0.28)";
    ctx.shadowBlur = 8 * scale;
    ctx.shadowOffsetY = 2 * scale;
    ctx.fillStyle = "#FFFFFF";
    ctx.fill();
    ctx.shadowColor = "transparent";
  } else {
    ctx.globalAlpha = 0.72;
    ctx.fillStyle = "#FFFFFF";
    ctx.fill();
    ctx.setLineDash([4 * scale, 3 * scale]);
  }
  ctx.beginPath();
  ctx.roundRect(b.x + lw / 2, b.y + lw / 2, b.w - lw, b.h - lw, Math.max(0, l.radius - lw / 2));
  ctx.lineWidth = lw;
  ctx.strokeStyle = color;
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.fillStyle = BUBBLE_TEXT;
  ctx.font = l.font;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  l.lines.forEach((line, i) => ctx.fillText(line, b.x + l.padX, b.y + l.padY + l.lineH * (i + 0.5)));
  ctx.restore();
}

export interface CalloutOptions {
  style: CalloutStyle;
  /** Selection the bubbles must stay inside, physical px. */
  crop: Rect;
  /** Callout whose comment input is open: the input stands in for its bubble. */
  hideId?: number | null;
}

export interface PlacedCallout {
  callout: Callout;
  layout: BubbleLayout;
}

/** Callouts whose badge is inside `crop`, in list order; a captioned rectangle's stroked frame is its `avoid`. */
function calloutsToPlace(list: readonly Annotation[], scale: number, crop: Rect): Callout[] {
  const out: Callout[] = [];
  for (const a of list) {
    const c = calloutOf(a);
    if (!c || !markInCrop(c, crop)) continue;
    out.push(a.kind === "rect" ? { ...c, avoid: inflate(a.r, (a.size * scale) / 2) } : c);
  }
  return out;
}

/**
 * Bubbles of the callouts that belong to `crop`, laid out together: each one keeps off the other badges, the
 * `avoid` areas and the bubbles placed before it. Callouts outside the crop get none (their badge is cut
 * off). A callout whose badge is inside a captioned frame ignores that frame and still gets its text there.
 */
export function layoutCallouts(list: readonly Annotation[], env: Pick<RenderEnv, "scale" | "letter" | "measure">, crop: Rect): PlacedCallout[] {
  const callouts = calloutsToPlace(list, env.scale, crop);
  const badges = callouts.map((c) => markBounds(c, env));
  const placed: PlacedCallout[] = [];
  callouts.forEach((callout, i) => {
    // A rectangle's own badge sits on its corner, inside the inflated frame: its frame always counts.
    const frames = callouts.flatMap((c, j) => (c.avoid && (j === i || !contains(c.avoid, callout.p)) ? [c.avoid] : []));
    const obstacles = [...badges.filter((_, j) => j !== i), ...frames, ...placed.map((p) => p.layout.box)];
    const layout = bubbleLayout(callout, env, crop, obstacles);
    if (layout) placed.push({ callout, layout });
  });
  return placed;
}

/** Comment bubbles of the callouts; drawn after every annotation so the text stays on top. */
export function drawCallouts(ctx: Ctx2D, list: readonly Annotation[], env: RenderEnv, opts: CalloutOptions): void {
  // The hidden callout is still laid out, so the other bubbles do not jump when its input closes.
  for (const { callout, layout } of layoutCallouts(list, env, opts.crop)) {
    if (callout.id !== opts.hideId) drawBubble(ctx, layout, callout.color, opts.style, env.scale);
  }
}

function drawText(ctx: Ctx2D, a: TextAnn, env: RenderEnv): void {
  const l = textLayout(a, env.scale, env.measure);
  if (a.plate) {
    ctx.fillStyle = a.color;
    ctx.beginPath();
    ctx.roundRect(l.box.x, l.box.y, l.box.w, l.box.h, l.px * 0.25);
    ctx.fill();
  }
  ctx.fillStyle = a.plate ? contrastText(a.color) : a.color;
  ctx.font = l.font;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.fillText(a.text, l.box.x + l.padX, l.box.y + l.padY + l.lineH / 2);
}

export function drawAnnotation(ctx: Ctx2D, a: Annotation, env: RenderEnv): void {
  ctx.save();
  switch (a.kind) {
    case "arrow":
      drawArrow(ctx, a, env);
      break;
    case "pencil":
      drawPencil(ctx, a.points, a.size * env.scale, a.color);
      break;
    case "rect": {
      ctx.strokeStyle = a.color;
      ctx.lineWidth = a.size * env.scale;
      ctx.lineJoin = "miter";
      ctx.strokeRect(a.r.x, a.r.y, a.r.w, a.r.h);
      const caption = calloutOf(a);
      if (caption) drawCaptionBadge(ctx, caption, env);
      break;
    }
    case "marker":
      ctx.globalCompositeOperation = "multiply";
      ctx.globalAlpha = MARKER_OPACITY;
      ctx.strokeStyle = a.color;
      ctx.lineWidth = markerWidth(a.size, env.scale);
      ctx.lineCap = "butt";
      strokeLine(ctx, a.a, a.b);
      break;
    case "blur":
      drawBlur(ctx, a, env);
      break;
    case "mark":
      drawMark(ctx, a, env);
      break;
    case "text":
      drawText(ctx, a, env);
      break;
  }
  ctx.restore();
}

export function drawAnnotations(ctx: Ctx2D, list: readonly Annotation[], env: RenderEnv, skipId: number | null = null): void {
  for (const a of list) if (a.id !== skipId) drawAnnotation(ctx, a, env);
}
