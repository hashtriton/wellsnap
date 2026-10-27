// Zoom, opacity and size of a pinned screenshot window. The window shows the whole image, so its size follows
// from the image, the zoom and whether it is folded to a small preview.

/** Wheel and +/- step through these, like a browser zoom. */
export const ZOOM_STEPS = [0.2, 0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
export const MIN_OPACITY = 0.1;
/** Long side of the folded preview in CSS px. */
export const PREVIEW_SIDE = 96;
/** The core's smallest window side, physical px. */
const MIN_SIDE = 16;

/** The next zoom step above (dir 1) or below (dir -1) `zoom`; a zoom between steps snaps to the next one. */
export function stepZoom(zoom: number, dir: 1 | -1): number {
  const eps = 1e-6;
  if (dir > 0) return ZOOM_STEPS.find((z) => z > zoom + eps) ?? ZOOM_STEPS[ZOOM_STEPS.length - 1];
  return [...ZOOM_STEPS].reverse().find((z) => z < zoom - eps) ?? ZOOM_STEPS[0];
}

/** `step` applied `times` times (a wheel event worth several notches), at most 10. */
export function repeatStep(value: number, times: number, step: (v: number) => number): number {
  let next = value;
  for (let i = 0; i < Math.min(times, 10); i++) next = step(next);
  return next;
}

/** Opacity in tenths, from 10% to 100%. */
export function stepOpacity(opacity: number, dir: 1 | -1): number {
  const next = Math.round(opacity * 10 + dir) / 10;
  return Math.min(1, Math.max(MIN_OPACITY, next));
}

export interface Size {
  width: number;
  height: number;
}

/** Physical window size for an image of `image` px at `zoom`, or folded to the preview. */
export function windowSize(image: Size, zoom: number, folded: boolean, dpr: number): Size {
  if (folded) {
    const long = Math.max(image.width, image.height, 1);
    const k = Math.min(1, (PREVIEW_SIDE * dpr) / long);
    return { width: Math.max(MIN_SIDE, Math.round(image.width * k)), height: Math.max(MIN_SIDE, Math.round(image.height * k)) };
  }
  return { width: Math.max(MIN_SIDE, Math.round(image.width * zoom)), height: Math.max(MIN_SIDE, Math.round(image.height * zoom)) };
}

/** The zoom a window of `width` physical px shows the image at (after the system rescaled it on another monitor). */
export function zoomOf(image: Size, width: number): number {
  return image.width > 0 ? width / image.width : 1;
}

/** "150%". */
export function percent(value: number): string {
  return `${Math.round(value * 100)}%`;
}
