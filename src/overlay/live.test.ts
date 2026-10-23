import { describe, expect, it, vi } from "vitest";
import type { View } from "./editor";
import { liveRegion } from "./live";

vi.mock("../shared/ipc", () => ({ api: {} }));

const view = (over: Partial<View>): View =>
  ({ settled: true, scale: 1, selection: { x: 100, y: 50, w: 200, h: 120 }, ...over }) as View;

describe("live region", () => {
  it("keeps the whole frame while there is no chosen area", () => {
    expect(liveRegion(view({ selection: null }), [])).toBeNull();
    // Being chosen, moved or resized.
    expect(liveRegion(view({ settled: false }), [])).toBeNull();
  });

  it("keeps the area with a band for its handles, and the controls with their corners", () => {
    const toolbar = { x: 100, y: 180, w: 300, h: 40, r: 10 };
    expect(liveRegion(view({}), [toolbar])).toEqual([
      { x: 92, y: 42, w: 216, h: 136, r: 0 },
      { x: 100, y: 180, w: 300, h: 40, r: 10 },
    ]);
  });

  it("covers fractional CSS boxes in physical px on a scaled monitor", () => {
    const region = liveRegion(view({ scale: 1.5, selection: { x: 10.2, y: 10, w: 20, h: 20 } }), [{ x: 0.5, y: 0.5, w: 10.1, h: 9, r: 999 }]);
    expect(region).toEqual([
      { x: 3, y: 3, w: 55, h: 54, r: 0 },
      // A pill: the radius stops at half the height.
      { x: 0, y: 0, w: 16, h: 15, r: 7 },
    ]);
  });
});
