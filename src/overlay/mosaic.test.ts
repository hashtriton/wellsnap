import { describe, expect, it } from "vitest";
import { computeMosaic, mosaicCellSize, type PixelSource } from "./mosaic";
import type { Rect } from "./types";

const SECRET: [number, number, number] = [0, 255, 0];

/** Gray image (with a gentle gradient) whose `secret` areas are pure green; records every read. */
function source(width: number, height: number, secrets: Rect[]) {
  const reads: [number, number][] = [];
  const inSecret = (x: number, y: number) => secrets.some((r) => x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h);
  const src: PixelSource = {
    width,
    height,
    read(x, y, out) {
      if (x < 0 || y < 0 || x >= width || y >= height) throw new Error(`read outside the image: ${x},${y}`);
      reads.push([x, y]);
      if (inSecret(x, y)) {
        out[0] = SECRET[0];
        out[1] = SECRET[1];
        out[2] = SECRET[2];
      } else {
        out[0] = 100 + (x % 20);
        out[1] = 110;
        out[2] = 120 + (y % 20);
      }
    },
  };
  return { src, reads, inSecret };
}

function colors(data: Uint8ClampedArray): [number, number, number][] {
  const out: [number, number, number][] = [];
  for (let i = 0; i < data.length; i += 4) out.push([data[i], data[i + 1], data[i + 2]]);
  return out;
}

describe("mosaic", () => {
  it("never reads a pixel inside the region and never shows its color", () => {
    const region = { x: 40, y: 30, w: 60, h: 45 };
    const { src, reads, inSecret } = source(160, 120, [region]);
    const grid = computeMosaic(src, region, 8);
    expect(reads.length).toBeGreaterThan(0);
    expect(reads.filter(([x, y]) => inSecret(x, y))).toEqual([]);
    for (const [r, g, b] of colors(grid.data)) {
      expect([r, g, b]).not.toEqual(SECRET);
      // Gray surroundings plus noise: green never dominates.
      expect(g - r).toBeLessThan(60);
      expect(g - b).toBeLessThan(60);
    }
  });

  it("treats fractional regions as covering every touched pixel", () => {
    const region = { x: 40.6, y: 30.2, w: 59.1, h: 44.5 };
    const { src, reads } = source(160, 120, []);
    computeMosaic(src, region, 8);
    const touched = (x: number, y: number) => x >= 40 && x < 100 && y >= 30 && y < 75;
    expect(reads.filter(([x, y]) => touched(x, y))).toEqual([]);
  });

  it("skips other hidden regions so neighbouring boxes do not leak", () => {
    const left = { x: 20, y: 20, w: 40, h: 30 };
    const right = { x: 60, y: 20, w: 40, h: 30 };
    const { src, reads, inSecret } = source(140, 90, [left, right]);
    const grid = computeMosaic(src, right, 8, [left]);
    expect(reads.filter(([x, y]) => inSecret(x, y))).toEqual([]);
    for (const c of colors(grid.data)) expect(c).not.toEqual(SECRET);
  });

  it("works at the image edges and falls back to gray when no ring is left", () => {
    const { src, reads } = source(50, 40, []);
    const edge = computeMosaic(src, { x: 0, y: 0, w: 30, h: 20 }, 6);
    expect(reads.every(([x, y]) => x >= 0 && y >= 0)).toBe(true);
    expect(edge.cols).toBe(5);
    expect(edge.rows).toBe(4);

    const full = computeMosaic(source(50, 40, []).src, { x: 0, y: 0, w: 50, h: 40 }, 10);
    for (const [r, g, b] of colors(full.data)) {
      expect(Math.abs(r - 128)).toBeLessThanOrEqual(24);
      expect(Math.abs(g - 128)).toBeLessThanOrEqual(24);
      expect(Math.abs(b - 128)).toBeLessThanOrEqual(24);
    }
  });

  it("is deterministic, so the screen and the export match", () => {
    const region = { x: 10, y: 10, w: 30, h: 30 };
    const a = computeMosaic(source(60, 60, []).src, region, 6);
    const b = computeMosaic(source(60, 60, []).src, region, 6);
    expect(Array.from(a.data)).toEqual(Array.from(b.data));
  });

  it("varies cells with noise", () => {
    const grid = computeMosaic(source(200, 200, []).src, { x: 50, y: 50, w: 100, h: 100 }, 10);
    expect(new Set(colors(grid.data).map((c) => c.join(","))).size).toBeGreaterThan(20);
  });
});

describe("mosaicCellSize", () => {
  it("is a sixth of the shorter side, capped at 16 css px and at least 6", () => {
    expect(mosaicCellSize({ x: 0, y: 0, w: 600, h: 60 }, 1)).toBe(10);
    expect(mosaicCellSize({ x: 0, y: 0, w: 600, h: 600 }, 1)).toBe(16);
    expect(mosaicCellSize({ x: 0, y: 0, w: 600, h: 600 }, 2)).toBe(32);
    expect(mosaicCellSize({ x: 0, y: 0, w: 20, h: 20 }, 1)).toBe(6);
  });
});
