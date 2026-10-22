import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { events, type PreparePayload } from "../shared/ipc";

export interface FrameTimings {
  fetchMs: number;
  drawMs: number;
}

interface SharedBufferEvent extends Event {
  getBuffer(): ArrayBuffer;
  additionalData: unknown;
}

interface WebView2Bridge {
  addEventListener(type: "sharedbufferreceived", listener: (e: SharedBufferEvent) => void): void;
  releaseBuffer(buffer: ArrayBuffer): void;
}

function webview2(): WebView2Bridge | undefined {
  return (window as unknown as { chrome?: { webview?: WebView2Bridge } }).chrome?.webview;
}

/**
 * Subscribes to frames for this overlay. On Windows the pixels arrive through WebView2 shared memory
 * together with the payload; otherwise only the payload arrives and the pixels are fetched from `wsnap`.
 * `onFrame` must draw the pixels synchronously: shared memory is released right after it returns.
 */
export function listenForFrames(onFrame: (payload: PreparePayload, rgba: Uint8ClampedArray) => void): void {
  const bridge = webview2();
  bridge?.addEventListener("sharedbufferreceived", (e) => {
    const buffer = e.getBuffer();
    try {
      onFrame(e.additionalData as PreparePayload, new Uint8ClampedArray(buffer));
    } finally {
      bridge.releaseBuffer(buffer);
    }
  });
  // Tell the core this page can receive frames (it waits for that after creating a window), but only once the
  // listener is registered: otherwise the first prepare can arrive before it and the capture times out.
  void events
    .onPrepare(async (payload) => {
      const response = await fetch(convertFileSrc(payload.framePath, "wsnap"), { cache: "no-store" });
      if (!response.ok) throw new Error(`frame request failed: ${response.status}`);
      onFrame(payload, new Uint8ClampedArray(await response.arrayBuffer()));
    })
    .then(() => invoke("overlay_page_loaded"));
}

/** Draws RGBA pixels 1:1 into `canvas` (canvas size = physical pixels of the monitor). */
export function drawFrame(canvas: HTMLCanvasElement, rgba: Uint8ClampedArray, width: number, height: number): void {
  if (rgba.length !== width * height * 4) {
    throw new Error(`unexpected frame size ${rgba.length} for ${width}x${height}`);
  }
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) throw new Error("2d context unavailable");
  ctx.putImageData(new ImageData(rgba as Uint8ClampedArray<ArrayBuffer>, width, height), 0, 0);
}
