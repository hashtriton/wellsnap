import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SheetPutMeta, SheetRequest, Snapshot } from "../shared/ipc";
import { composeSheet, startComposer, type ComposerDeps } from "./composer";

type Call = { name: string; args: unknown[] };
let calls: Call[] = [];
let canvases: { w: number; h: number }[] = [];
let bitmaps: { closed: boolean; options: unknown }[] = [];
let fetched: string[] = [];
let status = 200;

/** 2D context that records every method call; measureText follows the font size like the layout tests. */
function recordingContext(): unknown {
  const props: Record<string, unknown> = {};
  return new Proxy(props, {
    get(target, name: string) {
      if (name in target) return target[name];
      if (name === "measureText") {
        return (text: string) => ({ width: text.length * Number(/([\d.]+)px/.exec(String(target.font))?.[1] ?? 10) * 0.55 });
      }
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
    canvases.push({ w, h });
  }
  getContext() {
    return recordingContext();
  }
  async convertToBlob() {
    return { arrayBuffer: async () => new Uint8Array([0x89, 0x50, 0x4e, 0x47]).buffer };
  }
}

function snap(letter: string, comment = ""): Snapshot {
  return { letter, width: 1920, height: 1080, marks: comment ? [{ num: 1, comment }] : [], commentsOnImage: false, revision: 0, scale: 1 };
}

function request(requestId: number, snapshots = [snap("A", "Кнопка"), snap("B")]): SheetRequest {
  return { requestId, sessionId: "s1", title: "Снимок", withComments: true, snapshots };
}

const url = (sessionId: string, letter: string, revision: number) => `wsnap://snap/${sessionId}/${letter}/${revision}`;
const texts = () => calls.filter((c) => c.name === "fillText").map((c) => c.args[0]);
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Deps whose `listen` hands out the event callback, so a test can deliver requests. */
function fakeDeps(pending: SheetRequest | null = null) {
  const order: string[] = [];
  let deliver: (r: SheetRequest) => void = () => undefined;
  const unlisten = vi.fn();
  const deps = {
    listen: vi.fn(async (cb: (r: SheetRequest) => void) => {
      order.push("listen");
      deliver = cb;
      return unlisten;
    }),
    pending: vi.fn(async () => {
      order.push("pending");
      return pending;
    }),
    put: vi.fn(async (_png: Uint8Array, _meta: SheetPutMeta) => undefined),
    fail: vi.fn(async (_id: number, _reason: string) => undefined),
    metric: vi.fn(async (_name: string, _value: number) => undefined),
    imageUrl: url,
  } satisfies ComposerDeps;
  return { deps, order, unlisten, send: (r: SheetRequest) => deliver(r) };
}

beforeEach(() => {
  calls = [];
  canvases = [];
  bitmaps = [];
  fetched = [];
  status = 200;
  vi.stubGlobal("OffscreenCanvas", FakeOffscreenCanvas);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (u: string) => {
      fetched.push(u);
      return { ok: status === 200, status, blob: async () => ({}) };
    }),
  );
  vi.stubGlobal(
    "createImageBitmap",
    vi.fn(async (_blob: unknown, options: unknown) => {
      const bitmap = {
        closed: false,
        options,
        close: (): void => {
          bitmap.closed = true;
        },
      };
      bitmaps.push(bitmap);
      return bitmap;
    }),
  );
});
afterEach(() => vi.unstubAllGlobals());

describe("composeSheet", () => {
  it("draws every snapshot once, scaled to its cell, with headings and comments", async () => {
    const { bytes, plan } = await composeSheet(request(1), url);
    expect(bytes).toEqual(new Uint8Array([0x89, 0x50, 0x4e, 0x47]));
    expect(canvases).toContainEqual({ w: plan.width, h: plan.height });
    expect(fetched).toEqual(["wsnap://snap/s1/A/0", "wsnap://snap/s1/B/0"]);
    const draws = calls.filter((c) => c.name === "drawImage");
    expect(draws.map((c) => c.args.slice(1))).toEqual(plan.cells.map((c) => [c.image.x, c.image.y, c.image.w, c.image.h]));
    expect(bitmaps.map((b) => b.options)).toEqual(plan.cells.map((c) => ({ resizeWidth: c.image.w, resizeHeight: c.image.h, resizeQuality: "high" })));
    expect(bitmaps.every((b) => b.closed)).toBe(true);
    expect(texts()).toEqual(["Снимок A", "A1", "Кнопка", "Снимок B"]);
    // White background first, and a border around each picture.
    expect(calls[0]).toEqual({ name: "fillRect", args: [0, 0, plan.width, plan.height] });
    expect(calls.filter((c) => c.name === "strokeRect")).toHaveLength(2);
  });

  it("draws no comments for smart paste's sheet", async () => {
    await composeSheet({ ...request(1), withComments: false }, url);
    expect(texts()).toEqual(["Снимок A", "Снимок B"]);
  });

  it("fails on a snapshot the core no longer serves", async () => {
    status = 404;
    await expect(composeSheet(request(1), url)).rejects.toThrow("HTTP 404");
  });
});

describe("startComposer", () => {
  it("subscribes before it asks for the pending request, then answers it", async () => {
    const { deps, order } = fakeDeps(request(3));
    startComposer(deps);
    await vi.waitFor(() => expect(deps.put).toHaveBeenCalledTimes(1));
    expect(order).toEqual(["listen", "pending"]);
    const [png, meta] = deps.put.mock.calls[0];
    expect(png).toBeInstanceOf(Uint8Array);
    expect(meta).toEqual({ requestId: 3, width: canvases[1].w, height: canvases[1].h, cramped: false });
    expect(deps.fail).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(deps.metric).toHaveBeenCalledWith("sheet.ms", expect.any(Number)));
  });

  it("reports a failed fetch to the core", async () => {
    status = 404;
    const { deps, send } = fakeDeps();
    startComposer(deps);
    await flush();
    send(request(5));
    await vi.waitFor(() => expect(deps.fail).toHaveBeenCalledWith(5, "snapshot A: HTTP 404"));
    expect(deps.put).not.toHaveBeenCalled();
  });

  it("reports a put the core refused, so no waiter sits out the timeout", async () => {
    const { deps, send } = fakeDeps();
    deps.put.mockRejectedValueOnce(new Error("the set changed"));
    startComposer(deps);
    await flush();
    send(request(4));
    await vi.waitFor(() => expect(deps.fail).toHaveBeenCalledWith(4, "the set changed"));
    expect(deps.metric).not.toHaveBeenCalled();
  });

  it("ignores a request that is not newer than the last one", async () => {
    const { deps, send } = fakeDeps(request(7));
    startComposer(deps);
    await vi.waitFor(() => expect(deps.put).toHaveBeenCalledTimes(1));
    send(request(7));
    send(request(6));
    await flush();
    send(request(8, [snap("C")]));
    await vi.waitFor(() => expect(deps.put).toHaveBeenCalledTimes(2));
    expect(deps.put.mock.calls.map((c) => c[1].requestId)).toEqual([7, 8]);
  });

  it("answers one request at a time", async () => {
    const { deps, send } = fakeDeps();
    let release: () => void = () => undefined;
    deps.put.mockImplementationOnce(() => new Promise<undefined>((resolve) => (release = () => resolve(undefined))));
    startComposer(deps);
    await flush();
    send(request(1));
    await vi.waitFor(() => expect(deps.put).toHaveBeenCalledTimes(1));
    send(request(2));
    await flush();
    expect(deps.put).toHaveBeenCalledTimes(1);
    release();
    await vi.waitFor(() => expect(deps.put).toHaveBeenCalledTimes(2));
  });

  it("stops listening when stopped", async () => {
    const { deps, unlisten, send } = fakeDeps();
    const stop = startComposer(deps);
    await flush();
    stop();
    await flush();
    expect(unlisten).toHaveBeenCalled();
    send(request(9));
    await flush();
    expect(deps.put).not.toHaveBeenCalled();
  });
});
