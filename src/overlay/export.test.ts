import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { exportImage, finishMeta } from "./export";
import type { Measure, RenderEnv } from "./render";
import type { Annotation } from "./types";

const measure: Measure = (text, font) => text.length * Number(/([\d.]+)px/.exec(font)?.[1] ?? 10) * 0.55;
const env: RenderEnv = { scale: 1, letter: null, measure, mosaic: () => null };

type Call = { name: string; args: unknown[] };
let calls: Call[] = [];
let canvasSize: { w: number; h: number } | null = null;

/** 2D context that records every method call and accepts any property. */
function recordingContext(): unknown {
  const props: Record<string, unknown> = {};
  return new Proxy(props, {
    get(target, name: string) {
      if (name in target) return target[name];
      return (...args: unknown[]) => {
        calls.push({ name, args });
      };
    },
    set(target, name: string, value) {
      target[name] = value;
      return true;
    },
  });
}

class FakeOffscreenCanvas {
  constructor(w: number, h: number) {
    canvasSize = { w, h };
  }
  getContext() {
    return recordingContext();
  }
  async convertToBlob() {
    return { arrayBuffer: async () => new ArrayBuffer(4) };
  }
}

const annotations: Annotation[] = [
  { kind: "mark", id: 1, color: "#FF6A1A", num: 1, p: { x: 150, y: 150 }, tail: { x: 300, y: 260 }, comment: "Кнопка не нажимается" },
  { kind: "mark", id: 2, color: "#E53935", num: 2, p: { x: 400, y: 300 }, tail: null, comment: "" },
  { kind: "arrow", id: 3, color: "#E53935", size: 4, a: { x: 120, y: 400 }, b: { x: 300, y: 420 } },
];

const texts = () => calls.filter((c) => c.name === "fillText").map((c) => c.args[0]);

describe("exportImage", () => {
  beforeEach(() => {
    calls = [];
    canvasSize = null;
    vi.stubGlobal("OffscreenCanvas", FakeOffscreenCanvas);
  });
  afterEach(() => vi.unstubAllGlobals());

  const run = (commentsOnImage: boolean) =>
    exportImage({
      frame: {} as CanvasImageSource,
      crop: { x: 100, y: 100, w: 600, h: 400 },
      annotations,
      env,
      commentsOnImage,
      format: "png",
      quality: 0.9,
    });

  it("draws the comment bubbles when the comments go on the image", async () => {
    const result = await run(true);
    expect(texts()).toContain("Кнопка не нажимается");
    expect(texts()).toEqual(expect.arrayContaining(["1", "2"]));
    expect(result).toMatchObject({ width: 600, height: 400 });
  });

  it("draws no bubble for a callout whose badge is outside the crop", async () => {
    const outside: Annotation = { kind: "mark", id: 9, color: "#FF6A1A", num: 3, p: { x: 50, y: 50 }, tail: { x: 200, y: 200 }, comment: "orphan" };
    await exportImage({
      frame: {} as CanvasImageSource,
      crop: { x: 100, y: 100, w: 600, h: 400 },
      annotations: [...annotations, outside],
      env,
      commentsOnImage: true,
      format: "png",
      quality: 0.9,
    });
    expect(texts()).toContain("Кнопка не нажимается");
    expect(texts()).not.toContain("orphan");
  });

  it("keeps only badges and pointers when the text goes separately", async () => {
    const result = await run(false);
    expect(texts()).not.toContain("Кнопка не нажимается");
    expect(texts()).toEqual(expect.arrayContaining(["1", "2"]));
    expect(calls.some((c) => c.name === "roundRect")).toBe(true);
    // No band under the picture: the image is exactly the selection.
    expect(canvasSize).toEqual({ w: 600, h: 400 });
    expect(result).toMatchObject({ width: 600, height: 400 });
  });
});

describe("captions in the export", () => {
  beforeEach(() => {
    calls = [];
    vi.stubGlobal("OffscreenCanvas", FakeOffscreenCanvas);
  });
  afterEach(() => vi.unstubAllGlobals());

  const captioned: Annotation[] = [
    { kind: "arrow", id: 1, color: "#E53935", size: 4, a: { x: 150, y: 150 }, b: { x: 400, y: 300 }, caption: { num: 1, comment: "Кнопка не реагирует" } },
    { kind: "rect", id: 2, color: "#1E88E5", size: 4, r: { x: 200, y: 350, w: 200, h: 100 }, caption: { num: 2, comment: "Тут пусто" } },
  ];
  const run = (commentsOnImage: boolean) =>
    exportImage({
      frame: {} as CanvasImageSource,
      crop: { x: 100, y: 100, w: 600, h: 400 },
      annotations: captioned,
      env: { ...env, letter: "A" },
      commentsOnImage,
      format: "png",
      quality: 0.9,
    });

  it("draws the caption text with the comments on the image", async () => {
    await run(true);
    expect(texts()).toEqual(expect.arrayContaining(["A1", "A2", "Кнопка не реагирует", "Тут пусто"]));
  });

  it("keeps only the labels when the text goes separately", async () => {
    await run(false);
    expect(texts()).toEqual(["A1", "A2"]);
  });

  it("sends the captions as marks", () => {
    const meta = finishMeta({
      captureId: 1,
      action: "copy",
      letter: "A",
      format: "png",
      placement: "separate",
      annotations: [...captioned, ...annotations.map((a) => (a.kind === "mark" ? { ...a, id: a.id + 10, num: a.num + 2 } : { ...a, id: a.id + 10 }))],
      crop: { x: 100, y: 100, w: 600, h: 400 },
      image: { width: 600, height: 400 },
      scale: 1,
    });
    expect(meta.marks).toEqual([
      { num: 1, comment: "Кнопка не реагирует" },
      { num: 2, comment: "Тут пусто" },
      { num: 3, comment: "Кнопка не нажимается" },
      { num: 4, comment: "" },
    ]);
    expect(meta.commentsOnImage).toBe(false);
  });
});

describe("finishMeta", () => {
  const crop = { x: 100, y: 100, w: 600, h: 400 };
  const base = { captureId: 7, action: "copy" as const, letter: "B", format: "png" as const, annotations, crop, image: { width: 600, height: 400 }, scale: 1.5 };

  it("tells the core whether the comments are on the image", () => {
    expect(finishMeta({ ...base, placement: "onImage" }).commentsOnImage).toBe(true);
    expect(finishMeta({ ...base, placement: "separate" }).commentsOnImage).toBe(false);
  });

  it("carries the marks in number order and the image size", () => {
    expect(finishMeta({ ...base, placement: "separate" })).toEqual({
      captureId: 7,
      action: "copy",
      width: 600,
      height: 400,
      letter: "B",
      marks: [
        { num: 1, comment: "Кнопка не нажимается" },
        { num: 2, comment: "" },
      ],
      commentsOnImage: false,
      format: "png",
      scale: 1.5,
      tools: { mark: 2, arrow: 1 },
    });
  });

  it("counts the annotations per tool", () => {
    expect(finishMeta({ ...base, placement: "separate", annotations: [] }).tools).toEqual({});
    const blur: Annotation = { kind: "blur", id: 8, color: "#000", r: { x: 0, y: 0, w: 10, h: 10 }, mode: "mosaic" };
    expect(finishMeta({ ...base, placement: "separate", annotations: [...annotations, blur, { ...blur, id: 9 }] }).tools).toEqual({
      mark: 2,
      arrow: 1,
      blur: 2,
    });
  });

  it("passes the monitor scale and the finish hint to the core", () => {
    expect(finishMeta({ ...base, placement: "onImage", scale: 2 })).toMatchObject({ scale: 2 });
    expect(finishMeta({ ...base, placement: "onImage" }).hint).toBeUndefined();
    const meta = finishMeta({ ...base, placement: "onImage", hint: "noUpload" });
    expect(JSON.parse(JSON.stringify(meta))).toMatchObject({ scale: 1.5, hint: "noUpload" });
  });

  it("leaves out callouts whose badge is outside the crop", () => {
    const outside: Annotation = { kind: "mark", id: 9, color: "#FF6A1A", num: 3, p: { x: 50, y: 50 }, tail: { x: 200, y: 200 }, comment: "outside" };
    const meta = finishMeta({ ...base, placement: "separate", annotations: [...annotations, outside] });
    expect(meta.marks.map((m) => m.num)).toEqual([1, 2]);
  });
});
