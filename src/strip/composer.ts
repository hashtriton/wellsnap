// Composes one-picture sheets for the core: it asks with `sheet:request`, the strip answers with the PNG
// (`sheet_put`) or the reason it could not (`sheet_failed`). One request at a time; the core never has more
// than one in flight, and ids only grow, so an id that is not newer than the last one is a repeat.
import type { Measure } from "../overlay/render";
import type { SheetPutMeta, SheetRequest } from "../shared/ipc";
import { planSheet, SHEET_BACKGROUND, SHEET_BORDER, SHEET_TEXT, type SheetPlan } from "./sheetLayout";

export interface ComposerDeps {
  /** Subscribes to `sheet:request`; resolves to the unsubscribe function. */
  listen: (cb: (request: SheetRequest) => void) => Promise<() => void>;
  /** The request sent before the subscription, if it is still open. */
  pending: () => Promise<SheetRequest | null>;
  put: (png: Uint8Array, meta: SheetPutMeta) => Promise<void>;
  fail: (requestId: number, reason: string) => Promise<void>;
  metric: (name: string, value: number) => Promise<void>;
  imageUrl: (sessionId: string, letter: string, revision: number) => string;
}

/** Starts answering requests; returns the function that stops it. */
export function startComposer(deps: ComposerDeps): () => void {
  let stopped = false;
  let last = 0;
  let next: SheetRequest | null = null;
  let running = false;

  const pump = async () => {
    if (running) return;
    running = true;
    try {
      while (next && !stopped) {
        const request = next;
        next = null;
        await answer(request, deps);
      }
    } finally {
      running = false;
    }
  };
  const offer = (request: SheetRequest) => {
    if (stopped || request.requestId <= last) return;
    last = request.requestId;
    next = request;
    void pump();
  };

  const unlisten = deps.listen(offer);
  // Subscribed first: a request sent from now on arrives as an event, an earlier one comes from here.
  unlisten
    .then(() => deps.pending())
    .then((request) => {
      if (request) offer(request);
    })
    .catch(() => undefined);
  return () => {
    stopped = true;
    unlisten.then((off) => off()).catch(() => undefined);
  };
}

async function answer(request: SheetRequest, deps: ComposerDeps): Promise<void> {
  const started = performance.now();
  try {
    const { bytes, plan } = await composeSheet(request, deps.imageUrl);
    await deps.put(bytes, { requestId: request.requestId, width: plan.width, height: plan.height, cramped: plan.cramped });
    deps.metric("sheet.ms", performance.now() - started).catch(() => undefined);
  } catch (err) {
    // Also after a refused put: the core has usually ended the request then, and ignores this repeat.
    await deps.fail(request.requestId, err instanceof Error ? err.message : String(err)).catch(() => undefined);
  }
}

/** Fetches, scales and draws every snapshot of the request, one at a time, and encodes the sheet as PNG. */
export async function composeSheet(
  request: SheetRequest,
  imageUrl: ComposerDeps["imageUrl"],
): Promise<{ bytes: Uint8Array; plan: SheetPlan }> {
  const plan = planSheet(request.snapshots, { title: request.title, withComments: request.withComments, measure: canvasMeasure() });
  const canvas = new OffscreenCanvas(plan.width, plan.height);
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) throw new Error("2d context unavailable");
  ctx.fillStyle = SHEET_BACKGROUND;
  ctx.fillRect(0, 0, plan.width, plan.height);
  ctx.strokeStyle = SHEET_BORDER;
  ctx.lineWidth = 1;
  ctx.fillStyle = SHEET_TEXT;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";

  for (const cell of plan.cells) {
    const snap = request.snapshots.find((s) => s.letter === cell.letter);
    if (!snap) throw new Error(`snapshot ${cell.letter} is missing`);
    // A CORS fetch of the wsnap protocol: a blob from it never taints the canvas.
    const response = await fetch(imageUrl(request.sessionId, snap.letter, snap.revision));
    if (!response.ok) throw new Error(`snapshot ${snap.letter}: HTTP ${response.status}`);
    const { x, y, w, h } = cell.image;
    // Scaled once, straight to the final size; closed right away so only one full frame is in memory.
    const bitmap = await createImageBitmap(await response.blob(), { resizeWidth: w, resizeHeight: h, resizeQuality: "high" });
    try {
      ctx.drawImage(bitmap, x, y, w, h);
    } finally {
      bitmap.close();
    }
    // Outside the picture, so a white screenshot keeps its edge on the white sheet.
    ctx.strokeRect(x - 0.5, y - 0.5, w + 1, h + 1);
    for (const t of [cell.heading, ...cell.lines]) {
      ctx.font = t.font;
      ctx.fillText(t.text, t.x, t.y + t.height / 2);
    }
  }

  const blob = await canvas.convertToBlob({ type: "image/png" });
  return { bytes: new Uint8Array(await blob.arrayBuffer()), plan };
}

/** Text widths from a canvas, cached: the layout search measures the same words many times. */
function canvasMeasure(): Measure {
  const ctx = new OffscreenCanvas(1, 1).getContext("2d");
  if (!ctx) throw new Error("2d context unavailable");
  const widths = new Map<string, number>();
  return (text, font) => {
    const key = `${font}\n${text}`;
    let width = widths.get(key);
    if (width === undefined) {
      ctx.font = font;
      width = ctx.measureText(text).width;
      widths.set(key, width);
    }
    return width;
  };
}
