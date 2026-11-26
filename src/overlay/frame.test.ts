import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({ invoke: vi.fn(() => Promise.resolve()), convertFileSrc: vi.fn((p: string) => p) }));
const ipc = vi.hoisted(() => ({ events: { onPrepare: vi.fn() } }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("../shared/ipc", () => ipc);

import { listenForFrames } from "./frame";
import type { PreparePayload } from "../shared/ipc";

describe("listenForFrames", () => {
  beforeEach(() => {
    vi.stubGlobal("window", {});
    core.invoke.mockClear();
  });
  afterEach(() => vi.unstubAllGlobals());

  it("reports the page as loaded only after the prepare listener is registered", async () => {
    let registered: () => void = () => {};
    ipc.events.onPrepare.mockReturnValueOnce(new Promise<() => void>((resolve) => (registered = () => resolve(() => {}))));
    listenForFrames(() => {});
    await Promise.resolve();
    expect(core.invoke).not.toHaveBeenCalled();
    registered();
    await vi.waitFor(() => expect(core.invoke).toHaveBeenCalledWith("overlay_page_loaded"));
  });

  it("keeps a late frame from replacing the next capture", async () => {
    let resolveOld: (bytes: ArrayBuffer) => void = () => {};
    const oldBytes = new Promise<ArrayBuffer>((resolve) => (resolveOld = resolve));
    const readOld = vi.fn(() => oldBytes);
    const readNew = vi.fn(async () => new ArrayBuffer(4));
    const fetchFrame = vi.fn()
      .mockResolvedValueOnce({ ok: true, arrayBuffer: readOld })
      .mockResolvedValueOnce({ ok: true, arrayBuffer: readNew });
    vi.stubGlobal("fetch", fetchFrame);
    ipc.events.onPrepare.mockResolvedValueOnce(() => {});
    const onFrame = vi.fn();
    listenForFrames(onFrame);
    const receive: (payload: PreparePayload) => Promise<void> = ipc.events.onPrepare.mock.lastCall![0];
    const old = { captureId: 7, framePath: "frame/7/overlay-0" } as PreparePayload;
    const current = { ...old, captureId: 8, framePath: "frame/8/overlay-0" };

    const pending = receive(old);
    await vi.waitFor(() => expect(readOld).toHaveBeenCalledOnce());
    await receive(current);
    resolveOld(new ArrayBuffer(4));
    await pending;

    expect(onFrame).toHaveBeenCalledExactlyOnceWith(current, new Uint8ClampedArray(4));
  });
});
