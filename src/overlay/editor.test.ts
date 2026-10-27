// Editor behaviour against a minimal DOM stand-in (the tests run in node): key auto-repeat, focus on canvas
// clicks, failed finish, ready reporting and callouts outside the selection.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PreparePayload, Settings } from "../shared/ipc";
import type { Editor as EditorType } from "./editor";

const api = vi.hoisted(() => ({
  overlayReady: vi.fn(() => Promise.resolve()),
  overlayCancel: vi.fn(() => Promise.resolve()),
  overlayFinish: vi.fn((): Promise<void> => Promise.resolve()),
  logMetric: vi.fn(() => Promise.resolve()),
  updateSettings: vi.fn(() => Promise.resolve()),
}));
vi.mock("../shared/ipc", () => ({ api }));
vi.mock("./frame", () => ({ drawFrame: vi.fn() }));

type Listener = (e: unknown) => void;

function listeners() {
  const map = new Map<string, Listener[]>();
  return {
    add: (type: string, fn: Listener) => map.set(type, [...(map.get(type) ?? []), fn]),
    fire: (type: string, e: unknown) => map.get(type)?.forEach((fn) => fn(e)),
  };
}

/** 2D context that accepts every call; reads that the editor uses return plausible values. */
function fakeContext(): unknown {
  const props: Record<string, unknown> = {
    measureText: (text: string) => ({ width: text.length * 7 }),
    getImageData: (_x: number, _y: number, w: number, h: number) => ({ data: new Uint8ClampedArray(w * h * 4) }),
  };
  return new Proxy(props, {
    get: (target, name: string) => (name in target ? target[name] : () => {}),
    set: (target, name: string, value) => {
      target[name] = value;
      return true;
    },
  });
}

class FakeCanvas {
  width = 1;
  height = 1;
  style = { cursor: "" };
  events = listeners();
  getContext = () => fakeContext();
  addEventListener = (type: string, fn: Listener) => this.events.add(type, fn);
  setPointerCapture() {}
  releasePointerCapture() {}
  hasPointerCapture = () => false;
}

class FakeOffscreenCanvas {
  getContext = () => fakeContext();
  convertToBlob = async () => ({ arrayBuffer: async () => new ArrayBuffer(4) });
}

class FakeElement {
  blur = vi.fn();
}
class FakeButton extends FakeElement {}
class FakeInput extends FakeElement {}
class FakeTextArea extends FakeElement {}

let windowEvents = listeners();
let frames: FrameRequestCallback[] = [];
const doc = { activeElement: null as unknown, createElement: () => new FakeCanvas() };

function flushFrames(): void {
  const run = frames;
  frames = [];
  run.forEach((cb) => cb(0));
}

const payload: PreparePayload = {
  captureId: 1,
  label: "overlay-0",
  monitor: { id: "0", name: "Test", x: 0, y: 0, width: 800, height: 600, scale: 1 },
  transport: "shared",
  framePath: "",
  width: 800,
  height: 600,
  setLetter: null,
  setCount: 0,
  setFull: false,
  captureMs: 1,
};

let Editor: typeof EditorType;
let canvas: FakeCanvas;
let editor: EditorType;

const pointer = (x: number, y: number) => ({ button: 0, clientX: x, clientY: y, pointerId: 1, shiftKey: false, preventDefault() {} });

function drag(from: [number, number], to: [number, number]): void {
  canvas.events.fire("pointerdown", pointer(...from));
  canvas.events.fire("pointermove", pointer(...to));
  canvas.events.fire("pointerup", pointer(...to));
}

function key(code: string, repeat = false, keyName = code, extra: { shiftKey?: boolean; ctrlKey?: boolean; target?: unknown } = {}): void {
  windowEvents.fire("keydown", {
    code,
    key: keyName,
    repeat,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    target: {},
    preventDefault() {},
    ...extra,
  });
}

/** A press and a release without moves in between; `upAt` is where the release lands. */
function click(x: number, y: number, upAt: [number, number] = [x, y]): void {
  canvas.events.fire("pointerdown", pointer(x, y));
  canvas.events.fire("pointerup", pointer(...upAt));
}

/** What the browser sends for a double-click: two clicks, then the dblclick. `jitter` shifts the first release. */
function dblclick(x: number, y: number, jitter = 0): void {
  click(x, y, [x + jitter, y]);
  click(x, y);
  canvas.events.fire("dblclick", { clientX: x, clientY: y });
}

beforeEach(async () => {
  vi.useFakeTimers();
  windowEvents = listeners();
  frames = [];
  doc.activeElement = null;
  vi.stubGlobal("navigator", { platform: "Win32", userAgent: "" });
  vi.stubGlobal("document", doc);
  vi.stubGlobal("window", {
    innerWidth: 800,
    innerHeight: 600,
    devicePixelRatio: 1,
    addEventListener: windowEvents.add,
    setTimeout: (fn: () => void, ms?: number) => setTimeout(fn, ms),
    clearTimeout: (id?: number) => clearTimeout(id),
  });
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => frames.push(cb));
  vi.stubGlobal("cancelAnimationFrame", () => {});
  vi.stubGlobal("OffscreenCanvas", FakeOffscreenCanvas);
  vi.stubGlobal("HTMLButtonElement", FakeButton);
  vi.stubGlobal("HTMLInputElement", FakeInput);
  vi.stubGlobal("HTMLTextAreaElement", FakeTextArea);
  Object.values(api).forEach((fn) => fn.mockClear());
  ({ Editor } = await import("./editor"));
  canvas = new FakeCanvas();
  editor = new Editor(canvas as unknown as HTMLCanvasElement);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** Frame shown, selection 100..300, one plain rectangle drawn with the rect tool (still active). */
function withAnnotation(): void {
  editor.onFrame(payload, new Uint8ClampedArray(0));
  drag([100, 100], [300, 300]);
  editor.setTool("rect");
  drag([150, 150], [250, 250]);
  // The caption input that opens after drawing, closed empty: the rectangle stays plain.
  editor.closeInput();
  expect(editor.getView()).toMatchObject({ canUndo: true, input: null, markCount: 0 });
}

describe("ready reporting", () => {
  it("reports ready after the new frame is committed, not synchronously", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    expect(api.overlayReady).not.toHaveBeenCalled();
    flushFrames();
    vi.advanceTimersByTime(0);
    expect(api.overlayReady).toHaveBeenCalledTimes(1);
    expect(api.overlayReady).toHaveBeenCalledWith(1);
    vi.advanceTimersByTime(100);
    expect(api.overlayReady).toHaveBeenCalledTimes(1);
  });

  it("reports ready from the fallback timer when no frame comes", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    vi.advanceTimersByTime(49);
    expect(api.overlayReady).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(api.overlayReady).toHaveBeenCalledTimes(1);
    flushFrames();
    vi.advanceTimersByTime(0);
    expect(api.overlayReady).toHaveBeenCalledTimes(1);
  });

  it("does not report a frame that was reset before it was shown", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    editor.reset();
    flushFrames();
    vi.advanceTimersByTime(100);
    expect(api.overlayReady).not.toHaveBeenCalled();
  });
});

describe("key auto-repeat", () => {
  it("does not let a held Esc walk on to discarding the snapshot", () => {
    withAnnotation();
    key("Escape");
    expect(editor.getView().tool).toBe("select");
    for (let i = 0; i < 20; i++) key("Escape", true);
    expect(editor.getView().armed).toBe(false);
    expect(api.overlayCancel).not.toHaveBeenCalled();
    key("Escape");
    expect(editor.getView().armed).toBe(true);
    key("Escape", true);
    expect(api.overlayCancel).not.toHaveBeenCalled();
    key("Escape");
    expect(api.overlayCancel).toHaveBeenCalledTimes(1);
  });

  it("does not finish on a held Enter", () => {
    withAnnotation();
    key("Enter", true);
    expect(editor.getView().busy).toBeNull();
    expect(api.overlayFinish).not.toHaveBeenCalled();
  });

  it("still repeats nudges", () => {
    withAnnotation();
    const before = editor.getView().selection!;
    key("ArrowRight");
    key("ArrowRight", true);
    key("ArrowRight", true);
    expect(editor.getView().selection!.x).toBe(before.x + 3);
  });
});

describe("canvas click", () => {
  it("takes the focus away from a toolbar button", () => {
    withAnnotation();
    const button = new FakeButton();
    doc.activeElement = button;
    drag([160, 160], [170, 170]);
    expect(button.blur).toHaveBeenCalled();
  });
});

describe("failed finish", () => {
  it("keeps the snapshot open with the reason and allows a retry", async () => {
    withAnnotation();
    const selection = editor.getView().selection;
    api.overlayFinish.mockRejectedValueOnce("Access is denied. (os error 5)");
    await editor.finish("save");
    const view = editor.getView();
    expect(view.busy).toBeNull();
    expect(view.phase).toBe("ready");
    expect(view.selection).toEqual(selection);
    expect(view.canUndo).toBe(true);
    expect(view.notice).toEqual({ key: "overlay.notices.saveFailed", detail: "Access is denied. (os error 5)", tone: "error" });

    await editor.finish("copy");
    expect(api.overlayFinish).toHaveBeenCalledTimes(2);
  });

  it("names the action that failed", async () => {
    withAnnotation();
    api.overlayFinish.mockRejectedValueOnce(new Error("clipboard busy"));
    await editor.finish("copy");
    expect(editor.getView().notice).toMatchObject({ key: "overlay.notices.copyFailed", detail: "clipboard busy" });
    api.overlayFinish.mockRejectedValueOnce("");
    await editor.finish("addToSet");
    expect(editor.getView().notice).toEqual({ key: "overlay.notices.addFailed", detail: undefined, tone: "error" });
  });

  it("pins with the comments drawn on, the badges as on screen and the corner where it opens", async () => {
    withAnnotation();
    api.overlayFinish.mockRejectedValueOnce("too many pins");
    await editor.finish("pin");
    expect(editor.getView().notice).toMatchObject({ key: "overlay.notices.pinFailed", detail: "too many pins" });
    const meta = (api.overlayFinish.mock.calls[0] as unknown[])[1] as { action: string; letter: string | null; commentsOnImage: boolean; origin: number[] };
    expect(meta).toMatchObject({ action: "pin", letter: null, commentsOnImage: true });
    expect(meta.origin).toHaveLength(2);
  });

  it("stays quiet when the core already reset the overlay", async () => {
    withAnnotation();
    api.overlayFinish.mockImplementationOnce(() => {
      editor.reset();
      return Promise.reject("copy all failed");
    });
    await editor.finish("copyAll");
    expect(editor.getView().notice).toBeNull();
    expect(editor.getView().phase).toBe("empty");
  });
});

describe("callouts outside the selection", () => {
  it("are not created by a click outside the selection", () => {
    withAnnotation();
    editor.setTool("mark");
    drag([50, 50], [50, 50]);
    expect(editor.getView().markCount).toBe(0);
    drag([120, 120], [120, 120]);
    expect(editor.getView().markCount).toBe(1);
  });

  it("are reported when the selection moves away from them", () => {
    withAnnotation();
    editor.setTool("mark");
    drag([120, 120], [120, 120]);
    editor.closeInput();
    editor.setTool("select");
    drag([280, 280], [380, 380]);
    expect(editor.getView().selection).toMatchObject({ x: 200, y: 200 });
    expect(editor.getView().notice).toEqual({ key: "overlay.notices.marksOut", params: { count: 1 }, tone: "info" });
    drag([300, 300], [200, 200]);
    expect(editor.getView().notice).toBeNull();
  });

  it("are left out of the text block sent with the image", async () => {
    withAnnotation();
    editor.setTool("mark");
    drag([120, 120], [120, 120]);
    editor.closeInput();
    drag([280, 280], [280, 280]);
    editor.closeInput();
    editor.setTool("select");
    drag([110, 290], [210, 390]);
    await editor.finish("copy");
    const meta = (api.overlayFinish.mock.calls[0] as unknown[])[1] as { marks: { num: number }[] };
    expect(meta.marks.map((m) => m.num)).toEqual([2]);
  });
});

describe("captions after drawing", () => {
  type Meta = { marks: { num: number; comment: string }[]; commentsOnImage: boolean };
  const meta = (call = 0) => (api.overlayFinish.mock.calls[call] as unknown[])[1] as Meta;
  const lastPrefs = () => {
    const calls = api.updateSettings.mock.calls as unknown[][];
    return (calls[calls.length - 1][0] as { toolPrefs: { captions: unknown } }).toolPrefs;
  };
  const settings = (toolPrefs: Record<string, unknown>) =>
    ({ showHints: true, enterAction: "copy", commentPlacement: "onImage", imageFormat: "png", jpgQuality: 90, toolPrefs }) as unknown as Settings;

  function selectArea(): void {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    drag([100, 100], [300, 300]);
  }

  function captionsOn(tool: "arrow" | "rect"): void {
    editor.setTool(tool);
    expect(editor.getView().captions).toBe(true);
  }

  function captionsOff(tool: "arrow" | "rect"): void {
    editor.setTool(tool);
    key("KeyT", false, "T", { shiftKey: true });
    expect(editor.getView().captions).toBe(false);
  }

  it("are on by default", () => {
    selectArea();
    editor.setTool("arrow");
    expect(editor.getView().captions).toBe(true);
    drag([150, 150], [250, 250]);
    expect(editor.getView().input).toMatchObject({ kind: "mark", caption: true, stoppable: true, label: "1" });
    editor.closeInput();
    editor.setTool("select");
    expect(editor.getView().captions).toBeNull();
  });

  it("turn off with Shift+T for the active shape tool and are saved with the tool prefs", () => {
    selectArea();
    key("KeyT", false, "T", { shiftKey: true });
    expect(editor.getView().captions).toBeNull();
    vi.advanceTimersByTime(600);
    expect(api.updateSettings).not.toHaveBeenCalled();

    captionsOff("rect");
    editor.setTool("arrow");
    expect(editor.getView().captions).toBe(true);
    vi.advanceTimersByTime(499);
    expect(api.updateSettings).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(api.updateSettings).toHaveBeenCalledTimes(1);
    expect(lastPrefs().captions).toEqual({ arrow: true, rect: false });
    editor.setTool("rect");
    key("KeyT", false, "T", { shiftKey: true });
    expect(editor.getView().captions).toBe(true);
    drag([150, 150], [250, 250]);
    expect(editor.getView().input).toMatchObject({ caption: true });
  });

  it("leave the shape plain when turned off", () => {
    selectArea();
    captionsOff("arrow");
    drag([150, 150], [250, 250]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 0, canUndo: true });
  });

  it("put the shape and its text in one undo step and send the text as a mark", async () => {
    selectArea();
    captionsOn("arrow");
    drag([150, 150], [250, 250]);
    expect(editor.getView().input).toMatchObject({ kind: "mark", caption: true, stoppable: true, label: "1", value: "" });
    // Not a callout for the toolbar until it has text: the placement switch does not blink with every shape.
    expect(editor.getView().markCount).toBe(0);
    editor.setInputValue(" ");
    expect(editor.getView().markCount).toBe(0);
    editor.setInputValue("  Кнопка   не реагирует ");
    expect(editor.getView().markCount).toBe(1);
    editor.submitInput();
    expect(editor.getView()).toMatchObject({ input: null, markCount: 1, tool: "arrow" });
    editor.undo();
    expect(editor.getView()).toMatchObject({ canUndo: false, markCount: 0 });
    editor.redo();
    expect(editor.getView().markCount).toBe(1);
    await editor.finish("copy");
    expect(meta().marks).toEqual([{ num: 1, comment: "Кнопка не реагирует" }]);
    expect(meta().commentsOnImage).toBe(true);
  });

  it("leave the plain shape in the same step when the text stays empty", () => {
    selectArea();
    captionsOn("rect");
    drag([150, 150], [250, 250]);
    expect(editor.getView().input).toMatchObject({ caption: true, label: "1" });
    editor.closeInput();
    expect(editor.getView()).toMatchObject({ input: null, markCount: 0, canUndo: true });
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
    editor.redo();
    expect(editor.getView().markCount).toBe(0);
  });

  it("let a press while the input is open start the next shape", async () => {
    selectArea();
    captionsOn("arrow");
    drag([150, 150], [250, 250]);
    const first = editor.getView().input!.id;
    editor.setInputValue("first");
    drag([150, 200], [250, 280]);
    const view = editor.getView();
    expect(view.input).toMatchObject({ label: "2", value: "" });
    expect(view.input!.id).not.toBe(first);
    expect(view.markCount).toBe(1);
    // A late blur of the first input does not close the second one.
    editor.closeInput(first);
    expect(editor.getView().input).toMatchObject({ label: "2" });
    editor.setInputValue("second");
    expect(editor.getView().markCount).toBe(2);
    await editor.finish("copy");
    expect(meta().marks).toEqual([
      { num: 1, comment: "first" },
      { num: 2, comment: "second" },
    ]);
  });

  it("only commit on a press when the input was not opened by drawing", () => {
    selectArea();
    captionsOff("arrow");
    drag([150, 150], [250, 250]);
    dblclick(200, 200);
    expect(editor.getView().input).toMatchObject({ caption: true, stoppable: false, label: "1" });
    drag([150, 280], [250, 290]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 0 });
    // The press drew nothing: the arrow is the only step.
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
  });

  it("leave a shape shorter than the badge plain", () => {
    selectArea();
    captionsOn("arrow");
    drag([150, 150], [170, 160]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 0, canUndo: true, notice: null });
    captionsOn("rect");
    drag([150, 150], [175, 160]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 0 });
    drag([150, 150], [180, 160]);
    expect(editor.getView().input).not.toBeNull();
  });

  it("are not added to a shape that starts outside the selection", () => {
    withAnnotation();
    captionsOn("arrow");
    drag([50, 50], [200, 200]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 0 });
    expect(editor.getView().notice).toEqual({ key: "overlay.notices.captionOutside", tone: "info" });
    // Only the tip outside: the caption stays.
    drag([200, 200], [400, 400]);
    expect(editor.getView().input).toMatchObject({ label: "1" });
  });

  it("are not added past 99 callouts", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    drag([0, 0], [800, 600]);
    editor.setTool("mark");
    for (let i = 0; i < 99; i++) {
      const p: [number, number] = [20 + (i % 19) * 40, 20 + Math.floor(i / 19) * 40];
      drag(p, p);
      editor.closeInput();
    }
    expect(editor.getView().markCount).toBe(99);
    captionsOn("arrow");
    drag([20, 400], [300, 400]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 99 });
    expect(editor.getView().notice).toMatchObject({ key: "overlay.notices.marksLimit", params: { count: 99 } });
  });

  it("are added on double-click and leave no trace when they stay empty", () => {
    withAnnotation();
    editor.setTool("select");
    dblclick(150, 200);
    expect(editor.getView().input).toMatchObject({ caption: true, stoppable: false, label: "1" });
    editor.closeInput();
    expect(editor.getView()).toMatchObject({ markCount: 0, canRedo: false });
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
    editor.redo();

    dblclick(150, 200);
    editor.setInputValue("frame");
    editor.closeInput();
    expect(editor.getView().markCount).toBe(1);
    editor.undo();
    expect(editor.getView()).toMatchObject({ markCount: 0, canUndo: true });
  });

  it("share the numbering with marks", async () => {
    selectArea();
    editor.setTool("mark");
    drag([120, 120], [120, 120]);
    editor.setInputValue("mark");
    editor.closeInput();
    captionsOn("rect");
    drag([150, 150], [250, 250]);
    expect(editor.getView().input).toMatchObject({ label: "2" });
    editor.setInputValue("frame");
    editor.closeInput();
    editor.setTool("select");
    drag([120, 120], [120, 120]);
    key("Delete");
    expect(editor.getView().markCount).toBe(1);
    await editor.finish("copy");
    expect(meta().marks).toEqual([{ num: 1, comment: "frame" }]);
  });

  it("go into the separate text like the other callouts", async () => {
    selectArea();
    captionsOn("arrow");
    drag([150, 150], [250, 250]);
    editor.setInputValue("text");
    editor.submitInput();
    editor.setCommentPlacement("separate");
    await editor.finish("copy");
    expect(meta()).toMatchObject({ commentsOnImage: false, marks: [{ num: 1, comment: "text" }] });
  });

  it("open on Enter for a selected captioned shape; Enter on a plain shape finishes", () => {
    withAnnotation();
    captionsOn("arrow");
    drag([120, 200], [280, 200]);
    editor.setInputValue("arrow");
    editor.submitInput();
    editor.setTool("select");
    drag([200, 200], [200, 200]);
    key("Enter");
    expect(editor.getView().input).toMatchObject({ caption: true, stoppable: false, value: "arrow" });
    editor.closeInput();
    drag([150, 180], [150, 180]);
    key("Enter");
    expect(editor.getView().busy).toBe("copy");
  });

  it("stop after drawing from the input, keeping its text", async () => {
    selectArea();
    captionsOn("arrow");
    drag([150, 150], [250, 250]);
    editor.setInputValue("kept");
    // Shift+T typed in the input is text, not the switch.
    key("KeyT", false, "T", { shiftKey: true, target: new FakeInput() });
    expect(editor.getView().captions).toBe(true);
    editor.stopCaptions();
    expect(editor.getView()).toMatchObject({ input: null, captions: false, markCount: 1 });
    vi.advanceTimersByTime(500);
    expect(lastPrefs().captions).toEqual({ arrow: false, rect: true });
    drag([150, 200], [250, 280]);
    expect(editor.getView()).toMatchObject({ input: null, markCount: 1 });
    await editor.finish("copy");
    expect(meta().marks).toEqual([{ num: 1, comment: "kept" }]);
  });

  it("are not reverted by a settings event while the switch is being saved", async () => {
    selectArea();
    captionsOff("arrow");
    editor.applySettings(settings({}));
    expect(editor.getView().captions).toBe(false);
    vi.advanceTimersByTime(500);
    expect(api.updateSettings).toHaveBeenCalledTimes(1);
    // Sent but not confirmed yet: an event from before the save still carries the old switch.
    editor.applySettings(settings({}));
    expect(editor.getView().captions).toBe(false);
    for (let i = 0; i < 5; i++) await Promise.resolve();
    editor.applySettings(settings({ captions: { arrow: true, rect: false } }));
    expect(editor.getView().captions).toBe(true);
  });
});

describe("double-click on a shape", () => {
  it("adds a caption with the Callout tool, without the mark its first click placed", async () => {
    withAnnotation();
    editor.setTool("mark");
    dblclick(150, 200);
    expect(editor.getView().input).toMatchObject({ caption: true, stoppable: false, label: "1" });
    editor.setInputValue("frame");
    editor.closeInput();
    expect(editor.getView().markCount).toBe(1);
    await editor.finish("copy");
    expect(((api.overlayFinish.mock.calls[0] as unknown[])[1] as { marks: unknown[] }).marks).toEqual([{ num: 1, comment: "frame" }]);
  });

  it("leaves no trace of the stray mark in the history", () => {
    withAnnotation();
    editor.setTool("mark");
    dblclick(150, 200);
    editor.setInputValue("frame");
    editor.closeInput();
    editor.undo();
    expect(editor.getView()).toMatchObject({ markCount: 0, canUndo: true });
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
  });

  it("still places and opens a mark with the Callout tool where there is no shape", () => {
    withAnnotation();
    editor.setTool("mark");
    dblclick(200, 200);
    expect(editor.getView()).toMatchObject({ markCount: 1, input: { caption: false, label: "1" } });
  });

  it("adds a caption with the Select tool and no extra step for the clicks", () => {
    withAnnotation();
    editor.setTool("select");
    dblclick(150, 200);
    expect(editor.getView().input).toMatchObject({ caption: true, label: "1" });
    editor.closeInput();
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
  });

  it("takes back a tiny shape drawn by a shaky first click", () => {
    withAnnotation();
    editor.setTool("arrow");
    dblclick(150, 200, 4);
    expect(editor.getView().input).toMatchObject({ caption: true, label: "1" });
    editor.setInputValue("frame");
    editor.closeInput();
    editor.undo();
    expect(editor.getView()).toMatchObject({ markCount: 0, canUndo: true });
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
  });

  it("keeps a shape drawn before the double-click", () => {
    withAnnotation();
    editor.setTool("arrow");
    drag([170, 200], [174, 200]);
    dblclick(170, 200);
    // The tiny arrow was meant: it is on top and gets the caption, the rectangle stays plain.
    editor.setInputValue("arrow");
    editor.closeInput();
    editor.undo();
    editor.undo();
    expect(editor.getView().canUndo).toBe(true);
  });

  it("does not open marks or text with the shape tools", () => {
    withAnnotation();
    editor.setTool("mark");
    click(200, 200);
    editor.closeInput();
    editor.setTool("arrow");
    dblclick(200, 200);
    expect(editor.getView().input).toBeNull();
    editor.setTool("select");
    dblclick(200, 200);
    expect(editor.getView().input).toMatchObject({ caption: false, label: "1" });
  });
});

describe("annotation handles", () => {
  it("add no undo step for a click without a drag", () => {
    withAnnotation();
    editor.setTool("select");
    drag([150, 200], [150, 200]);
    drag([250, 250], [250, 250]);
    drag([250, 250], [251, 251]);
    editor.undo();
    expect(editor.getView().canUndo).toBe(false);
    editor.redo();
    drag([150, 200], [150, 200]);
    drag([250, 250], [270, 270]);
    editor.undo();
    expect(editor.getView().canUndo).toBe(true);
  });

  /** Selection 100..300 and a selected arrow from (150, 150) to (250, 250) with caption 1. */
  function selectedCaptionedArrow(): void {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    drag([100, 100], [300, 300]);
    editor.setTool("arrow");
    drag([150, 150], [250, 250]);
    editor.setInputValue("start");
    editor.submitInput();
    editor.setTool("select");
    click(200, 200);
  }

  it("make the whole caption badge of a selected arrow its start handle", () => {
    selectedCaptionedArrow();
    // On the badge ring, off the small square around the start point.
    canvas.events.fire("pointermove", pointer(137, 150));
    expect(canvas.style.cursor).toBe("pointer");
    canvas.events.fire("pointermove", pointer(200, 200));
    expect(canvas.style.cursor).toBe("move");
  });

  it("report a caption whose start was dragged out of the selection", () => {
    selectedCaptionedArrow();
    drag([150, 150], [50, 50]);
    expect(editor.getView().notice).toEqual({ key: "overlay.notices.marksOut", params: { count: 1 }, tone: "info" });
  });
});

describe("Lightshot habits", () => {
  const ctrl = (code: string, repeat = false) => key(code, repeat, code, { ctrlKey: true });
  const finishMetaOf = (call = 0) => (api.overlayFinish.mock.calls[call] as unknown[])[1] as { action: string; hint?: string };

  it("copies on Ctrl+D with a ready selection and tells the core there is no upload", async () => {
    withAnnotation();
    ctrl("KeyD");
    expect(editor.getView().busy).toBe("copy");
    await vi.waitFor(() => expect(api.overlayFinish).toHaveBeenCalledTimes(1));
    expect(finishMetaOf()).toMatchObject({ action: "copy", hint: "noUpload" });
  });

  it("does not send the hint with a plain Ctrl+C", async () => {
    withAnnotation();
    ctrl("KeyC");
    await vi.waitFor(() => expect(api.overlayFinish).toHaveBeenCalledTimes(1));
    expect(finishMetaOf().hint).toBeUndefined();
  });

  it("explains on Ctrl+D before anything is selected", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    ctrl("KeyD");
    expect(api.overlayFinish).not.toHaveBeenCalled();
    expect(editor.getView().notice).toEqual({
      key: "overlay.notices.noUpload",
      params: { copy: "Ctrl+C", paste: "Ctrl+V" },
      tone: "info",
    });
  });

  it("points to the editing keys on Ctrl+E", () => {
    withAnnotation();
    ctrl("KeyE");
    expect(editor.getView().notice).toEqual({ key: "overlay.notices.editHere", tone: "info" });
    expect(editor.getView().phase).toBe("ready");
    expect(api.overlayFinish).not.toHaveBeenCalled();
  });

  it("shows the same notice on Ctrl+E before anything is selected", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    ctrl("KeyE");
    expect(editor.getView().notice).toEqual({ key: "overlay.notices.editHere", tone: "info" });
    expect(editor.getView().phase).toBe("idle");
    expect(api.overlayFinish).not.toHaveBeenCalled();
    expect(api.overlayCancel).not.toHaveBeenCalled();
  });

  it("closes on Ctrl+X with the same layers and second press as Esc", () => {
    withAnnotation();
    ctrl("KeyX");
    expect(editor.getView().tool).toBe("select");
    ctrl("KeyX");
    expect(editor.getView().armed).toBe(true);
    expect(api.overlayCancel).not.toHaveBeenCalled();
    ctrl("KeyX", true);
    expect(api.overlayCancel).not.toHaveBeenCalled();
    ctrl("KeyX");
    expect(api.overlayCancel).toHaveBeenCalledTimes(1);
  });

  it("closes an empty overlay on Ctrl+X at once", () => {
    editor.onFrame(payload, new Uint8ClampedArray(0));
    ctrl("KeyX");
    expect(api.overlayCancel).toHaveBeenCalledTimes(1);
  });

  it("ignores a held Ctrl+D and any habit while finishing", async () => {
    withAnnotation();
    ctrl("KeyD", true);
    expect(editor.getView().busy).toBeNull();
    ctrl("KeyS");
    expect(editor.getView().busy).toBe("save");
    ctrl("KeyX");
    ctrl("KeyD");
    expect(api.overlayCancel).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(api.overlayFinish).toHaveBeenCalledTimes(1));
    expect(finishMetaOf()).toMatchObject({ action: "save" });
  });
});
