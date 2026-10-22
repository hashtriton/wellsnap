// Hit testing, bounds and edits of annotations (pure; text and badge sizes come from `measure`).
import { contains, distToSegment, handlePoint, inflate, normalize, resizeRect, snap45, snapAxis, visibleHandles, type Handle } from "./geometry";
import { calloutOf } from "./marks";
import { arrowGeometry, markBounds, markerWidth, textLayout, type RenderEnv } from "./render";
import type { Annotation, Pt, Rect } from "./types";

export type AnnHandle = Handle | "a" | "b" | "tail";
type Env = Pick<RenderEnv, "scale" | "letter" | "measure">;

function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}

/** Badge of the caption of an arrow or a rectangle, null without one. */
function captionBadge(a: Annotation, env: Env): Rect | null {
  if (a.kind !== "arrow" && a.kind !== "rect") return null;
  const c = calloutOf(a);
  return c && markBounds(c, env);
}

export function bounds(a: Annotation, env: Env): Rect {
  const badge = captionBadge(a, env);
  switch (a.kind) {
    case "arrow":
    case "marker": {
      const pad = a.kind === "marker" ? markerWidth(a.size, env.scale) / 2 : arrowGeometry(a.a, a.b, a.size, env.scale).width;
      const r = inflate(normalize(a.a, a.b), pad);
      return badge ? union(r, badge) : r;
    }
    case "pencil": {
      let x0 = Infinity;
      let y0 = Infinity;
      let x1 = -Infinity;
      let y1 = -Infinity;
      for (const p of a.points) {
        x0 = Math.min(x0, p.x);
        y0 = Math.min(y0, p.y);
        x1 = Math.max(x1, p.x);
        y1 = Math.max(y1, p.y);
      }
      return inflate({ x: x0, y: y0, w: x1 - x0, h: y1 - y0 }, (a.size * env.scale) / 2);
    }
    case "rect":
    case "blur":
      return badge ? union(a.r, badge) : a.r;
    case "mark":
      return markBounds(a, env);
    case "text":
      return textLayout(a, env.scale, env.measure).box;
  }
}

/** Whether `p` touches the annotation; `tol` is the extra grab distance in physical px. */
export function hitTest(a: Annotation, p: Pt, env: Env, tol: number): boolean {
  const s = env.scale;
  const badge = captionBadge(a, env);
  if (badge && contains(inflate(badge, tol / 2), p)) return true;
  switch (a.kind) {
    case "arrow": {
      const g = arrowGeometry(a.a, a.b, a.size, s);
      return distToSegment(p, a.a, a.b) <= g.width / 2 + tol || Math.hypot(p.x - a.b.x, p.y - a.b.y) <= tol * 2;
    }
    case "marker":
      return distToSegment(p, a.a, a.b) <= markerWidth(a.size, s) / 2 + tol / 2;
    case "pencil": {
      const reach = (a.size * s) / 2 + tol;
      if (a.points.length === 1) return Math.hypot(p.x - a.points[0].x, p.y - a.points[0].y) <= reach;
      for (let i = 1; i < a.points.length; i++) if (distToSegment(p, a.points[i - 1], a.points[i]) <= reach) return true;
      return false;
    }
    case "rect": {
      const reach = (a.size * s) / 2 + tol;
      if (!contains(inflate(a.r, reach), p)) return false;
      const inner = inflate(a.r, -reach);
      return inner.w <= 0 || inner.h <= 0 || !contains(inner, p);
    }
    case "blur":
      return contains(a.r, p);
    case "mark":
      if (a.tail && distToSegment(p, a.p, a.tail) <= 3 * s + tol / 2) return true;
      return contains(inflate(markBounds(a, env), tol / 2), p);
    case "text":
      return contains(textLayout(a, s, env.measure).box, p);
  }
}

/** Topmost annotation under `p`. */
export function topHit(list: readonly Annotation[], p: Pt, env: Env, tol: number, kinds?: readonly Annotation["kind"][]): Annotation | null {
  for (let i = list.length - 1; i >= 0; i--) {
    const a = list[i];
    if (kinds && !kinds.includes(a.kind)) continue;
    if (hitTest(a, p, env, tol)) return a;
  }
  return null;
}

/**
 * Topmost annotation whose callout badge is under `p`: a mark (badge or pointer) or the caption badge of an
 * arrow or a rectangle. The Mark tool moves it instead of stacking a new mark on top.
 */
export function topCallout(list: readonly Annotation[], p: Pt, env: Env, tol: number): Annotation | null {
  for (let i = list.length - 1; i >= 0; i--) {
    const a = list[i];
    if (a.kind === "mark") {
      if (hitTest(a, p, env, tol)) return a;
      continue;
    }
    const badge = captionBadge(a, env);
    if (badge && contains(inflate(badge, tol / 2), p)) return a;
  }
  return null;
}

export function annotationHandles(a: Annotation, tol: number): { id: AnnHandle; p: Pt }[] {
  switch (a.kind) {
    case "arrow":
    case "marker":
      return [
        { id: "a", p: a.a },
        { id: "b", p: a.b },
      ];
    case "rect":
    case "blur": {
      // The caption badge sits on the top-left corner: dragging it moves the whole rectangle.
      const handles = visibleHandles(a.r, tol).filter((h) => !(h === "nw" && a.kind === "rect" && a.caption));
      return handles.map((h) => ({ id: h, p: handlePoint(a.r, h) }));
    }
    case "mark":
      return a.tail ? [{ id: "tail", p: a.tail }] : [];
    default:
      return [];
  }
}

export function hitAnnotationHandle(a: Annotation, p: Pt, tol: number, env: Env): AnnHandle | null {
  for (const h of annotationHandles(a, tol)) {
    if (Math.max(Math.abs(h.p.x - p.x), Math.abs(h.p.y - p.y)) <= tol) return h.id;
  }
  // The caption badge of an arrow sits on its start: all of it is the "a" handle, as far out as a press on
  // it would otherwise move the whole arrow (`topHit` at tol / 2 grows the badge by a quarter of `tol`).
  const badge = a.kind === "arrow" ? captionBadge(a, env) : null;
  if (badge && contains(inflate(badge, tol / 4), p)) return "a";
  return null;
}

const shift = (p: Pt, dx: number, dy: number): Pt => ({ x: p.x + dx, y: p.y + dy });

export function translate(a: Annotation, dx: number, dy: number): Annotation {
  switch (a.kind) {
    case "arrow":
    case "marker":
      return { ...a, a: shift(a.a, dx, dy), b: shift(a.b, dx, dy) };
    case "pencil":
      return { ...a, points: a.points.map((p) => shift(p, dx, dy)) };
    case "rect":
    case "blur":
      return { ...a, r: { ...a.r, x: a.r.x + dx, y: a.r.y + dy } };
    case "mark":
      return { ...a, p: shift(a.p, dx, dy), tail: a.tail && shift(a.tail, dx, dy) };
    case "text":
      return { ...a, p: shift(a.p, dx, dy) };
  }
}

/** Applies a handle drag of (dx, dy) to the annotation as it was when the drag started. */
export function dragHandle(orig: Annotation, h: AnnHandle, dx: number, dy: number, constrain: boolean): Annotation {
  switch (orig.kind) {
    case "arrow":
    case "marker": {
      const moving = h === "a" ? orig.a : orig.b;
      const fixed = h === "a" ? orig.b : orig.a;
      let p = shift(moving, dx, dy);
      if (constrain) p = orig.kind === "arrow" ? snap45(fixed, p) : snapAxis(fixed, p);
      return h === "a" ? { ...orig, a: p } : { ...orig, b: p };
    }
    case "rect":
    case "blur":
      if (h === "a" || h === "b" || h === "tail") return orig;
      return { ...orig, r: resizeRect(orig.r, h, dx, dy) };
    case "mark":
      return h === "tail" && orig.tail ? { ...orig, tail: shift(orig.tail, dx, dy) } : orig;
    default:
      return orig;
  }
}
