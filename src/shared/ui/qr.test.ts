import { describe, expect, it } from "vitest";
import { qrShape } from "./qr";

/** Rebuilds the module grid from the path to check the output geometry. */
function grid(shape: { size: number; path: string }): boolean[][] {
  const cells = Array.from({ length: shape.size }, () => Array<boolean>(shape.size).fill(false));
  for (const m of shape.path.matchAll(/M(\d+) (\d+)h(\d+)v1h-\d+z/g)) {
    const [x, y, w] = [Number(m[1]), Number(m[2]), Number(m[3])];
    for (let i = 0; i < w; i++) cells[y][x + i] = true;
  }
  return cells;
}

describe("qrShape", () => {
  it("draws finder patterns inside the quiet zone", () => {
    const shape = qrShape("https://pay.cloudtips.ru/p/12345678");
    const cells = grid(shape);
    const q = 4;
    const inner = shape.size - q * 2;
    expect(inner % 4).toBe(1);
    expect(inner).toBeGreaterThanOrEqual(21);
    // The 4-module quiet zone stays light on every side.
    for (let i = 0; i < q; i++) {
      for (const line of [cells[i], cells[shape.size - 1 - i], cells.map((r) => r[i]), cells.map((r) => r[shape.size - 1 - i])]) {
        expect(line.some(Boolean)).toBe(false);
      }
    }
    // Top-left finder: dark 7x7 border, light ring, dark 3x3 core.
    for (let i = 0; i < 7; i++) {
      expect(cells[q][q + i]).toBe(true);
      expect(cells[q + 6][q + i]).toBe(true);
      expect(cells[q + i][q]).toBe(true);
    }
    expect(cells[q + 1][q + 1]).toBe(false);
    expect(cells[q + 3][q + 3]).toBe(true);
    // Top-right finder.
    expect(cells[q][q + inner - 1]).toBe(true);
    expect(cells[q + 1][q + inner - 2]).toBe(false);
  });

  it("keeps a custom quiet zone", () => {
    expect(qrShape("abc", 2).size).toBe(qrShape("abc").size - 4);
  });

  it("grows with the data", () => {
    expect(qrShape("UQ" + "A".repeat(46)).size).toBeLessThan(qrShape("x".repeat(200)).size);
  });

  it("is deterministic", () => {
    expect(qrShape("abc")).toEqual(qrShape("abc"));
  });

  it("rejects empty and non-ASCII text", () => {
    expect(() => qrShape("")).toThrow();
    expect(() => qrShape("привет")).toThrow();
  });
});
