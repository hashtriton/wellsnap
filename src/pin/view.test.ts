import { describe, expect, it } from "vitest";
import { percent, repeatStep, stepOpacity, stepZoom, windowSize, zoomOf, ZOOM_STEPS } from "./view";

describe("pin zoom", () => {
  it("steps like a browser and stops at 20% and 500%", () => {
    expect(stepZoom(1, 1)).toBe(1.1);
    expect(stepZoom(1, -1)).toBe(0.9);
    expect(stepZoom(5, 1)).toBe(5);
    expect(stepZoom(0.2, -1)).toBe(0.2);
    expect(ZOOM_STEPS[0]).toBe(0.2);
  });

  it("takes several steps for one event of several notches, at most ten", () => {
    expect(repeatStep(1, 3, (z) => stepZoom(z, 1))).toBe(1.5);
    expect(repeatStep(1, 50, (z) => stepZoom(z, -1))).toBe(0.2);
    expect(repeatStep(0.5, 0, (z) => stepZoom(z, 1))).toBe(0.5);
  });

  it("snaps a zoom between steps to the next step in the wheel's direction", () => {
    // After the system rescaled the window on a monitor with another DPI.
    expect(stepZoom(0.8 * 1.25, 1)).toBe(1.1);
    expect(stepZoom(1.2, 1)).toBe(1.25);
    expect(stepZoom(1.2, -1)).toBe(1.1);
  });
});

describe("pin opacity", () => {
  it("steps in tenths between 10% and 100%", () => {
    expect(stepOpacity(1, -1)).toBe(0.9);
    expect(stepOpacity(1, 1)).toBe(1);
    expect(stepOpacity(0.1, -1)).toBe(0.1);
    expect(stepOpacity(0.3, 1)).toBe(0.4);
  });
});

describe("pin window size", () => {
  const image = { width: 400, height: 200 };

  it("is the image at its zoom, one image pixel per screen pixel at 100%", () => {
    expect(windowSize(image, 1, false, 2)).toEqual({ width: 400, height: 200 });
    expect(windowSize(image, 1.5, false, 1)).toEqual({ width: 600, height: 300 });
    expect(windowSize({ width: 3, height: 3 }, 1, false, 1)).toEqual({ width: 16, height: 16 });
  });

  it("folds to a preview of 96 css px on the long side, never larger than the image", () => {
    expect(windowSize(image, 3, true, 1)).toEqual({ width: 96, height: 48 });
    expect(windowSize(image, 1, true, 2)).toEqual({ width: 192, height: 96 });
    expect(windowSize({ width: 60, height: 30 }, 1, true, 1)).toEqual({ width: 60, height: 30 });
  });

  it("reads the zoom back from a rescaled window", () => {
    expect(zoomOf(image, 500)).toBe(1.25);
    expect(zoomOf({ width: 0, height: 0 }, 500)).toBe(1);
    expect(percent(1.25)).toBe("125%");
    expect(percent(0.333)).toBe("33%");
  });
});
