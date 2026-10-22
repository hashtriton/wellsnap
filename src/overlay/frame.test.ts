import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({ invoke: vi.fn(() => Promise.resolve()), convertFileSrc: vi.fn((p: string) => p) }));
const ipc = vi.hoisted(() => ({ events: { onPrepare: vi.fn() } }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("../shared/ipc", () => ipc);

import { listenForFrames } from "./frame";

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
});
