// Windows: once the area is chosen, the overlay keeps only the snapshot and its controls on screen, and the
// rest of the monitor is the live desktop again. The core clips the window to the rects sent from here.
// While the area is being chosen, moved or resized, the whole frozen frame is back: the edges are set
// against what the snapshot will contain.
import { api, type RegionRect } from "../shared/ipc";
import { HIT_CSS, type Editor, type View } from "./editor";
import type { Rect } from "./types";

/** A floating control in CSS px, with its corner radius. */
export interface UiBox extends Rect {
  r: number;
}

/**
 * Rects in physical px that stay on screen, or null for the whole frozen frame. The selection keeps a band
 * as wide as the handles' hit zone, so its handles stay visible and can be grabbed from outside.
 */
export function liveRegion(view: View, boxes: readonly UiBox[]): RegionRect[] | null {
  const sel = view.selection;
  if (!view.settled || !sel) return null;
  const s = view.scale;
  const crop = { x: sel.x - HIT_CSS, y: sel.y - HIT_CSS, w: sel.w + 2 * HIT_CSS, h: sel.h + 2 * HIT_CSS, r: 0 };
  return [crop, ...boxes].map((b) => {
    const x = Math.floor(b.x * s);
    const y = Math.floor(b.y * s);
    const w = Math.ceil((b.x + b.w) * s) - x;
    const h = Math.ceil((b.y + b.h) * s) - y;
    return { x, y, w, h, r: Math.min(Math.round(b.r * s), Math.floor(w / 2), Math.floor(h / 2)) };
  });
}

/** Floating controls under `root`: everything positioned on its own, except what is not shown yet. */
function uiBoxes(root: HTMLElement): UiBox[] {
  const boxes: UiBox[] = [];
  for (const el of root.querySelectorAll<HTMLElement>("*")) {
    const style = getComputedStyle(el);
    if (style.position !== "absolute" && style.position !== "fixed") continue;
    const b = el.getBoundingClientRect();
    if (b.width < 2 || b.height < 2 || unplaced(el, root)) continue;
    boxes.push({ x: b.left, y: b.top, w: b.width, h: b.height, r: parseFloat(style.borderTopLeftRadius) || 0 });
  }
  return boxes;
}

/** Inside a Floating box that is not placed yet (see Floating.tsx): it sits at 0,0 for a frame. */
function unplaced(el: HTMLElement, root: HTMLElement): boolean {
  for (let e: HTMLElement | null = el; e && e !== root; e = e.parentElement) {
    if (e.style.opacity === "0") return true;
  }
  return false;
}

/** Sends the region whenever the editor or the controls change; at most once a frame. */
export function watchLiveRegion(editor: Editor, root: HTMLElement): void {
  if (editor.getView().mac) return;
  // What the window shows now. The core gives every hidden overlay its whole frame back.
  let sent = "null";
  let raf = 0;
  const update = () => {
    raf = 0;
    const view = editor.getView();
    if (view.captureId === null) {
      sent = "null";
      return;
    }
    const region = liveRegion(view, uiBoxes(root));
    const key = region ? JSON.stringify([view.captureId, region]) : "null";
    if (key === sent) return;
    sent = key;
    api.overlayRegion(view.captureId, region).catch(() => {});
  };
  const schedule = () => {
    if (!raf) raf = requestAnimationFrame(update);
  };
  editor.subscribe(schedule);
  new MutationObserver(schedule).observe(root, { subtree: true, childList: true, attributes: true });
}
