// Irreversible "blur": a mosaic whose cell colors come only from a thin ring of pixels just outside the
// region plus deterministic noise. Pixels inside the region are never read, so nothing of the hidden
// content can be recovered (averaging pixelation and gaussian blur of text can be reversed). Same idea as
// Flameshot's pseudo-pixelation.
import type { Rect } from "./types";

export interface PixelSource {
  width: number;
  height: number;
  /** Writes the RGB of (x, y) into `out[0..2]`. Callers pass only coordinates inside the image. */
  read(x: number, y: number, out: number[]): void;
}

export interface MosaicGrid {
  cols: number;
  rows: number;
  cell: number;
  /** RGBA, `cols * rows` cells, row-major. */
  data: Uint8ClampedArray<ArrayBuffer>;
}

/** Width of the sampled band outside the region, in pixels. */
const RING = 3;
const NOISE = 18;
const JITTER = 6;

export function mosaicCellSize(r: Rect, scale: number): number {
  return Math.max(6, Math.floor(Math.min(Math.min(r.w, r.h) / 6, 16 * scale)));
}

interface IntRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** Outward rounding: the protected area always covers every pixel the mosaic is drawn on. */
function outward(r: Rect): IntRect {
  return { x0: Math.floor(r.x), y0: Math.floor(r.y), x1: Math.ceil(r.x + r.w), y1: Math.ceil(r.y + r.h) };
}

function inside(q: IntRect, x: number, y: number): boolean {
  return x >= q.x0 && x < q.x1 && y >= q.y0 && y < q.y1;
}

type Rgb = [number, number, number];

function hash(a: number, b: number, c: number): number {
  let h = Math.imul(a ^ 0x9e3779b9, 0x85ebca6b) ^ Math.imul(b ^ 0xc2b2ae35, 0x27d4eb2f) ^ Math.imul(c, 0x165667b1);
  h = Math.imul(h ^ (h >>> 15), 0x2c1b3c6d);
  h = Math.imul(h ^ (h >>> 12), 0x297a2d39);
  return ((h ^ (h >>> 15)) >>> 0) / 4294967296;
}

/**
 * Builds the mosaic for region `r`. `exclude` lists other hidden regions: their pixels are skipped too, so
 * two adjacent blur boxes never leak each other's content through their rings.
 */
export function computeMosaic(src: PixelSource, r: Rect, cell: number, exclude: readonly Rect[] = []): MosaicGrid {
  const q = outward(r);
  const blocked = [q, ...exclude.map(outward)];
  const cols = Math.max(1, Math.ceil((q.x1 - q.x0) / cell));
  const rows = Math.max(1, Math.ceil((q.y1 - q.y0) / cell));
  const px = [0, 0, 0];

  const allowed = (x: number, y: number) =>
    x >= 0 && y >= 0 && x < src.width && y < src.height && !blocked.some((b) => inside(b, x, y));

  const average = (xa: number, xb: number, ya: number, yb: number): Rgb | null => {
    let n = 0;
    let rs = 0;
    let gs = 0;
    let bs = 0;
    for (let y = ya; y < yb; y++) {
      for (let x = xa; x < xb; x++) {
        if (!allowed(x, y)) continue;
        src.read(x, y, px);
        rs += px[0];
        gs += px[1];
        bs += px[2];
        n++;
      }
    }
    return n ? [rs / n, gs / n, bs / n] : null;
  };

  const colSpan = (i: number) => [q.x0 + i * cell, Math.min(q.x0 + (i + 1) * cell, q.x1)] as const;
  const rowSpan = (j: number) => [q.y0 + j * cell, Math.min(q.y0 + (j + 1) * cell, q.y1)] as const;

  const top: (Rgb | null)[] = [];
  const bottom: (Rgb | null)[] = [];
  for (let i = 0; i < cols; i++) {
    const [xa, xb] = colSpan(i);
    top.push(average(xa, xb, q.y0 - RING, q.y0));
    bottom.push(average(xa, xb, q.y1, q.y1 + RING));
  }
  const left: (Rgb | null)[] = [];
  const right: (Rgb | null)[] = [];
  for (let j = 0; j < rows; j++) {
    const [ya, yb] = rowSpan(j);
    left.push(average(q.x0 - RING, q.x0, ya, yb));
    right.push(average(q.x1, q.x1 + RING, ya, yb));
  }
  const sides = [top, bottom, left, right].map(fillGaps);

  const seed = Math.imul(q.x0, 73856093) ^ Math.imul(q.y0, 19349663) ^ Math.imul(q.x1 - q.x0, 83492791) ^ (q.y1 - q.y0);
  const data = new Uint8ClampedArray(cols * rows * 4);
  for (let j = 0; j < rows; j++) {
    const v = (j + 0.5) / rows;
    for (let i = 0; i < cols; i++) {
      const u = (i + 0.5) / cols;
      const candidates: [Rgb | null, number][] = [
        [sides[0][i], v],
        [sides[1][i], 1 - v],
        [sides[2][j], u],
        [sides[3][j], 1 - u],
      ];
      let wsum = 0;
      const c = [0, 0, 0];
      for (const [color, dist] of candidates) {
        if (!color) continue;
        const w = 1 / ((dist + 0.05) * (dist + 0.05));
        c[0] += color[0] * w;
        c[1] += color[1] * w;
        c[2] += color[2] * w;
        wsum += w;
      }
      const base: Rgb = wsum ? [c[0] / wsum, c[1] / wsum, c[2] / wsum] : [128, 128, 128];
      const n = (hash(i, j, seed) * 2 - 1) * NOISE;
      const o = (j * cols + i) * 4;
      for (let k = 0; k < 3; k++) data[o + k] = base[k] + n + (hash(i, j, seed + k + 1) * 2 - 1) * JITTER;
      data[o + 3] = 255;
    }
  }
  return { cols, rows, cell, data };
}

/** Fills cells without samples (edge of the image, excluded areas) from the nearest sampled cell. */
function fillGaps(samples: (Rgb | null)[]): (Rgb | null)[] {
  if (samples.every((s) => s === null)) return samples;
  return samples.map((s, i) => {
    if (s) return s;
    for (let d = 1; d < samples.length; d++) {
      const near = samples[i - d] ?? samples[i + d];
      if (near) return near;
    }
    return null;
  });
}
