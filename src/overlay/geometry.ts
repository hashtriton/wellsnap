import type { Pt, Rect, Size } from "./types";

export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
export const HANDLES: readonly Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];
export type Side = "below" | "above" | "right" | "left";

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), hi);
}

export function normalize(a: Pt, b: Pt): Rect {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(b.x - a.x), h: Math.abs(b.y - a.y) };
}

export function contains(r: Rect, p: Pt): boolean {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

export function inflate(r: Rect, d: number): Rect {
  return { x: r.x - d, y: r.y - d, w: r.w + 2 * d, h: r.h + 2 * d };
}

export function clampPt(p: Pt, bounds: Size): Pt {
  return { x: clamp(p.x, 0, bounds.w), y: clamp(p.y, 0, bounds.h) };
}

export function scaleRect(r: Rect, k: number): Rect {
  return { x: r.x * k, y: r.y * k, w: r.w * k, h: r.h * k };
}

export function handlePoint(r: Rect, h: Handle): Pt {
  const cx = r.x + r.w / 2;
  const cy = r.y + r.h / 2;
  const right = r.x + r.w;
  const bottom = r.y + r.h;
  switch (h) {
    case "nw":
      return { x: r.x, y: r.y };
    case "n":
      return { x: cx, y: r.y };
    case "ne":
      return { x: right, y: r.y };
    case "e":
      return { x: right, y: cy };
    case "se":
      return { x: right, y: bottom };
    case "s":
      return { x: cx, y: bottom };
    case "sw":
      return { x: r.x, y: bottom };
    case "w":
      return { x: r.x, y: cy };
  }
}

/** Handles worth showing: side handles collapse into the corners on a small rectangle. */
export function visibleHandles(r: Rect, tol: number): Handle[] {
  return HANDLES.filter((h) => {
    if ((h === "n" || h === "s") && r.w < 2.5 * tol) return false;
    if ((h === "e" || h === "w") && r.h < 2.5 * tol) return false;
    return true;
  });
}

/** Nearest visible handle within `tol` (square hit zone), or null. */
export function hitHandle(r: Rect, p: Pt, tol: number): Handle | null {
  let best: Handle | null = null;
  let bestDist = Infinity;
  for (const h of visibleHandles(r, tol)) {
    const q = handlePoint(r, h);
    const d = Math.max(Math.abs(q.x - p.x), Math.abs(q.y - p.y));
    if (d <= tol && d < bestDist) {
      best = h;
      bestDist = d;
    }
  }
  return best;
}

/** Moves the edges named by `h` by (dx, dy); crossing the opposite edge flips the rectangle. */
export function resizeRect(orig: Rect, h: Handle, dx: number, dy: number, bounds?: Size): Rect {
  let left = orig.x;
  let top = orig.y;
  let right = orig.x + orig.w;
  let bottom = orig.y + orig.h;
  if (h.includes("w")) left += dx;
  if (h.includes("e")) right += dx;
  if (h.includes("n")) top += dy;
  if (h.includes("s")) bottom += dy;
  if (bounds) {
    left = clamp(left, 0, bounds.w);
    right = clamp(right, 0, bounds.w);
    top = clamp(top, 0, bounds.h);
    bottom = clamp(bottom, 0, bounds.h);
  }
  return normalize({ x: left, y: top }, { x: right, y: bottom });
}

/** Moves a rectangle, keeping it fully inside `bounds`. */
export function moveRect(orig: Rect, dx: number, dy: number, bounds: Size): Rect {
  return {
    x: clamp(orig.x + dx, 0, Math.max(0, bounds.w - orig.w)),
    y: clamp(orig.y + dy, 0, Math.max(0, bounds.h - orig.h)),
    w: orig.w,
    h: orig.h,
  };
}

export function cursorForHandle(h: Handle): string {
  switch (h) {
    case "nw":
    case "se":
      return "nwse-resize";
    case "ne":
    case "sw":
      return "nesw-resize";
    case "n":
    case "s":
      return "ns-resize";
    case "e":
    case "w":
      return "ew-resize";
  }
}

/** End point snapped to a multiple of 45 degrees around `a`. */
export function snap45(a: Pt, b: Pt): Pt {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len = Math.hypot(dx, dy);
  if (len === 0) return { ...b };
  const step = Math.PI / 4;
  const angle = Math.round(Math.atan2(dy, dx) / step) * step;
  return { x: a.x + Math.cos(angle) * len, y: a.y + Math.sin(angle) * len };
}

/** End point snapped to the horizontal or vertical through `a`, whichever is closer. */
export function snapAxis(a: Pt, b: Pt): Pt {
  return Math.abs(b.x - a.x) >= Math.abs(b.y - a.y) ? { x: b.x, y: a.y } : { x: a.x, y: b.y };
}

/** Corner opposite to `a` of the square spanned towards `b`. */
export function squareEnd(a: Pt, b: Pt): Pt {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const side = Math.max(Math.abs(dx), Math.abs(dy));
  return { x: a.x + (dx < 0 ? -side : side), y: a.y + (dy < 0 ? -side : side) };
}

export function distToSegment(p: Pt, a: Pt, b: Pt): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : clamp(((p.x - a.x) * dx + (p.y - a.y) * dy) / len2, 0, 1);
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

export interface ToolbarPlacement {
  x: number;
  y: number;
  orientation: "horizontal" | "vertical";
  side: Side | "inside";
}

/**
 * Places the toolbar next to the selection (all in CSS px): below, above, right (vertical), left (vertical),
 * else inside the bottom-right corner. Sides blocked by the monitor edge are skipped; the result never
 * leaves the monitor. The horizontal bar is right-aligned with the selection, where the drag usually ends.
 */
export function placeToolbar(sel: Rect, horiz: Size, vert: Size, screen: Size, gap = 8, margin = 8): ToolbarPlacement {
  const right = sel.x + sel.w;
  const bottom = sel.y + sel.h;
  const hx = clamp(right - horiz.w, margin, Math.max(margin, screen.w - margin - horiz.w));
  const vy = clamp(bottom - vert.h, margin, Math.max(margin, screen.h - margin - vert.h));
  const vertFits = vert.h <= screen.h - 2 * margin;

  if (bottom + gap + horiz.h <= screen.h - margin) return { x: hx, y: bottom + gap, orientation: "horizontal", side: "below" };
  if (sel.y - gap - horiz.h >= margin) return { x: hx, y: sel.y - gap - horiz.h, orientation: "horizontal", side: "above" };
  if (vertFits && right + gap + vert.w <= screen.w - margin) {
    return { x: right + gap, y: vy, orientation: "vertical", side: "right" };
  }
  if (vertFits && sel.x - gap - vert.w >= margin) {
    return { x: sel.x - gap - vert.w, y: vy, orientation: "vertical", side: "left" };
  }
  return {
    x: clamp(right - gap - horiz.w, margin, Math.max(margin, screen.w - margin - horiz.w)),
    y: clamp(bottom - gap - horiz.h, margin, Math.max(margin, screen.h - margin - horiz.h)),
    orientation: "horizontal",
    side: "inside",
  };
}

/**
 * Places a box of `size` next to `anchor`, trying the sides in `order`; the cross axis is centered on the
 * anchor and clamped to the screen. Falls back to the first side clamped into the screen.
 */
export function placeAround(
  anchor: Rect,
  size: Size,
  screen: Size,
  order: readonly Side[],
  gap = 8,
  margin = 8,
): { x: number; y: number; side: Side } {
  const cx = clamp(anchor.x + anchor.w / 2 - size.w / 2, margin, Math.max(margin, screen.w - margin - size.w));
  const cy = clamp(anchor.y + anchor.h / 2 - size.h / 2, margin, Math.max(margin, screen.h - margin - size.h));
  const at = (side: Side) => {
    switch (side) {
      case "below":
        return { x: cx, y: anchor.y + anchor.h + gap };
      case "above":
        return { x: cx, y: anchor.y - gap - size.h };
      case "right":
        return { x: anchor.x + anchor.w + gap, y: cy };
      case "left":
        return { x: anchor.x - gap - size.w, y: cy };
    }
  };
  for (const side of order) {
    const p = at(side);
    if (p.x >= margin && p.y >= margin && p.x + size.w <= screen.w - margin && p.y + size.h <= screen.h - margin) {
      return { ...p, side };
    }
  }
  const side = order[0] ?? "below";
  const p = at(side);
  return {
    x: clamp(p.x, margin, Math.max(margin, screen.w - margin - size.w)),
    y: clamp(p.y, margin, Math.max(margin, screen.h - margin - size.h)),
    side,
  };
}

/** Loupe position: below-right of the cursor, flipped on each axis where it would leave the screen. */
export function placeLoupe(cursor: Pt, size: Size, screen: Size, offset: number): Pt {
  let x = cursor.x + offset;
  if (x + size.w > screen.w) x = cursor.x - offset - size.w;
  let y = cursor.y + offset;
  if (y + size.h > screen.h) y = cursor.y - offset - size.h;
  return { x: clamp(x, 0, Math.max(0, screen.w - size.w)), y: clamp(y, 0, Math.max(0, screen.h - size.h)) };
}
