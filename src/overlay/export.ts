// Renders the selection at physical resolution: frozen pixels, annotations and, when the comments go on the
// image, the callout bubbles (same renderer as the screen), then encodes. The output is a flat raster:
// nothing under a blur survives.
import type { CommentPlacement, FinishAction, FinishMeta, ImageFormat } from "../shared/ipc";
import { marksMeta } from "./marks";
import { drawAnnotations, drawCallouts, type RenderEnv } from "./render";
import type { Annotation, Rect } from "./types";

export interface ExportRequest {
  frame: CanvasImageSource;
  crop: Rect;
  annotations: readonly Annotation[];
  env: RenderEnv;
  /** Draw the callout comments as bubbles; otherwise the image keeps only badges and pointers. */
  commentsOnImage: boolean;
  format: ImageFormat;
  /** JPEG quality 0..1. */
  quality: number;
}

export interface ExportResult {
  bytes: Uint8Array;
  width: number;
  height: number;
}

/** The pixel rectangle the image is cut from; the text block uses the same one to pick the callouts. */
export function exportCrop(r: Rect): Rect {
  return { x: Math.round(r.x), y: Math.round(r.y), w: Math.max(1, Math.round(r.w)), h: Math.max(1, Math.round(r.h)) };
}

export async function exportImage(req: ExportRequest): Promise<ExportResult> {
  const crop = exportCrop(req.crop);
  const canvas = new OffscreenCanvas(crop.w, crop.h);
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) throw new Error("2d context unavailable");
  ctx.drawImage(req.frame, crop.x, crop.y, crop.w, crop.h, 0, 0, crop.w, crop.h);
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, crop.w, crop.h);
  ctx.clip();
  ctx.translate(-crop.x, -crop.y);
  drawAnnotations(ctx, req.annotations, req.env);
  if (req.commentsOnImage) drawCallouts(ctx, req.annotations, req.env, { style: "solid", crop });
  ctx.restore();

  const blob =
    req.format === "jpg"
      ? await canvas.convertToBlob({ type: "image/jpeg", quality: req.quality })
      : await canvas.convertToBlob({ type: "image/png" });
  return { bytes: new Uint8Array(await blob.arrayBuffer()), width: crop.w, height: crop.h };
}

export interface MetaInput {
  captureId: number;
  action: FinishAction;
  letter: string | null;
  format: ImageFormat;
  placement: CommentPlacement;
  annotations: readonly Annotation[];
  /** Selection the image was cut from: callouts outside it are not in the text either. */
  crop: Rect;
  image: Pick<ExportResult, "width" | "height">;
}

/** Metadata for `overlay_finish`; with the comments on the image the core does not repeat them as text. */
export function finishMeta(m: MetaInput): FinishMeta {
  return {
    captureId: m.captureId,
    action: m.action,
    width: m.image.width,
    height: m.image.height,
    letter: m.letter,
    marks: marksMeta(m.annotations, exportCrop(m.crop)),
    commentsOnImage: m.placement === "onImage",
    format: m.format,
  };
}
