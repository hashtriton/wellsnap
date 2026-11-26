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
    await vi.waitFor(() => {
      expect(core.invoke).toHaveBeenCalledExactlyOnceWith("overlay_page_loaded", { shared: false, protocol: true });
    });
  });

  it("receives shared frames immediately and still discards them after a newer protocol frame", async () => {
    let registered: () => void = () => {};
    ipc.events.onPrepare.mockReturnValueOnce(new Promise<() => void>((resolve) => (registered = () => resolve(() => {}))));
    const addEventListener = vi.fn();
    const releaseBuffer = vi.fn();
    vi.stubGlobal("window", { chrome: { webview: { addEventListener, releaseBuffer } } });
    const onFrame = vi.fn();
    listenForFrames(onFrame);

    expect(addEventListener).toHaveBeenCalledExactlyOnceWith("sharedbufferreceived", expect.any(Function));
    expect(addEventListener.mock.invocationCallOrder[0]!).toBeLessThan(core.invoke.mock.invocationCallOrder[0]!);
    expect(core.invoke).toHaveBeenCalledExactlyOnceWith("overlay_page_loaded", { shared: true, protocol: false });
    const receiveShared = addEventListener.mock.lastCall![1];
    const buffer = new ArrayBuffer(4);
    const payload = { captureId: 7, transport: "shared" };
    receiveShared({ getBuffer: () => buffer, additionalData: payload });
    expect(onFrame).toHaveBeenCalledExactlyOnceWith(payload, new Uint8ClampedArray(buffer));
    expect(releaseBuffer).toHaveBeenCalledExactlyOnceWith(buffer);

    registered();
    await vi.waitFor(() => {
      expect(core.invoke).toHaveBeenNthCalledWith(2, "overlay_page_loaded", { shared: true, protocol: true });
    });

    const next = { captureId: 8, transport: "protocol", framePath: "frame/8/overlay-0" } as PreparePayload;
    const nextBytes = new ArrayBuffer(8);
    vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, arrayBuffer: async () => nextBytes })));
    const receiveProtocol: (payload: PreparePayload) => Promise<void> = ipc.events.onPrepare.mock.lastCall![0];
    await receiveProtocol(next);

    const staleBuffer = new ArrayBuffer(4);
    receiveShared({ getBuffer: () => staleBuffer, additionalData: payload });
    expect(onFrame).toHaveBeenCalledTimes(2);
    expect(onFrame).toHaveBeenLastCalledWith(next, new Uint8ClampedArray(nextBytes));
    expect(releaseBuffer).toHaveBeenLastCalledWith(staleBuffer);
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
