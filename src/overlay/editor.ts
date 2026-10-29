// Interaction controller of the overlay: owns the frozen frame, the crop selection, the annotations and
// their history, renders the canvas on demand (one rAF per change) and publishes a View for the React UI.
import type { Params } from "../shared/i18n";
import { api, type CommentPlacement, type FinishAction, type FinishHint, type ImageFormat, type PreparePayload, type Settings } from "../shared/ipc";
import { annotationHandles, bounds, dragHandle, hitAnnotationHandle, topCallout, topHit, translate, type AnnHandle } from "./annotations";
import { exportImage, finishMeta } from "./export";
import { drawFrame } from "./frame";
import {
  clampPt,
  contains,
  cursorForHandle,
  handlePoint,
  hitHandle,
  inflate,
  moveRect,
  nearestCorner,
  normalize,
  placeLoupe,
  resizeRect,
  scaleRect,
  snap45,
  snapAxis,
  squareEnd,
  visibleHandles,
  type Corner,
  type Handle,
} from "./geometry";
import { History } from "./history";
import { arrowDelta, escapeStep, formatChord, isRepeatable, matchHabit, matchKey, shortcutOf, type CommandId, type HabitId } from "./keys";
import { calloutOf, calloutsOf, MAX_MARKS, markInCrop, markLabel, nextMarkNum, removeAnnotation, setCaption } from "./marks";
import { computeMosaic, mosaicCellSize, type PixelSource } from "./mosaic";
import { defaultPrefs, parsePrefs, serializePrefs, SIZED_TOOLS, stepWidth, type ToolPrefs } from "./prefs";
import {
  BADGE_HEIGHT,
  drawAnnotation,
  drawAnnotations,
  drawCallouts,
  markBounds,
  markerWidth,
  textLayout,
  UI_FONT,
  type Measure,
  type MosaicTile,
  type RenderEnv,
} from "./render";
import type { Annotation, AnnotationKind, ArrowAnn, BlurAnn, BlurMode, DrawTool, Pt, Rect, RectAnn, Size, TextAnn, Tool } from "./types";

const DIM = "rgba(0, 0, 0, 0.4)";
const ACCENT = "#3366FF";
const GLASS = "rgba(22, 24, 30, 0.92)";
const MIN_SELECTION = 5;
const HANDLE_CSS = 8;
/** Half of the 16 px hit zone around handles. */
export const HIT_CSS = 8;
const LOUPE_PIXELS = 15;
const LOUPE_CELL_CSS = 10;
const SIZE_LABEL_MS = 1500;
const ARM_MS = 2000;
const PREFS_SAVE_MS = 500;
const NUDGE_MERGE_MS = 1000;
const TAIL_MIN_CSS = 18;
const WHEEL_STEP = 50;
const BRUSH_PREVIEW_MS = 700;
const CLICK_SLOP_CSS = 3;
const MOSAIC_CACHE = 64;
/** Upper bound on waiting for the new frame to be committed before the core is told to show the window. */
const READY_FALLBACK_MS = 50;

export type Phase = "empty" | "idle" | "creating" | "ready";

export interface InputView {
  /** "mark": a callout comment (a mark or the caption of an arrow or a rectangle); "text": free text. */
  kind: "mark" | "text";
  id: number;
  value: string;
  /** Badge text of the callout ("A1"), empty for text. */
  label: string;
  /** The callout is the caption of an arrow or a rectangle. */
  caption: boolean;
  /** Caption opened by drawing the shape: the input offers to stop opening captions. */
  stoppable: boolean;
  /** Badge (mark) or text box (text) in CSS px. */
  anchor: Rect;
  color: string;
  plate: boolean;
  /** Text only, CSS px. */
  fontPx: number;
  padX: number;
  padY: number;
  lineH: number;
  width: number;
}

export interface ToolContext {
  kind: AnnotationKind;
  /** "annotation": edits apply to the selected object; "tool": to the active tool's defaults. */
  target: "annotation" | "tool";
  color: string;
  size: number | null;
  mode: BlurMode | null;
  plate: boolean | null;
}

export interface Notice {
  key: string;
  params?: Params;
  /** Untranslated second line, such as the reason the core gave for a failed action. */
  detail?: string;
  tone: "info" | "error";
}

export interface View {
  phase: Phase;
  /** Capture the frame belongs to; null before the first frame and after a reset. */
  captureId: number | null;
  scale: number;
  /** Monitor size in CSS px. */
  screen: Size;
  /** Crop selection in CSS px. */
  selection: Rect | null;
  /** The selection is not being dragged: the toolbar may show. */
  settled: boolean;
  /** Corner of the selection where its last drag ended; the toolbar's primary button goes next to it. */
  corner: Corner;
  tool: Tool;
  context: ToolContext | null;
  canUndo: boolean;
  canRedo: boolean;
  /** Callouts: marks and captions. */
  markCount: number;
  commentPlacement: CommentPlacement;
  /** Caption after drawing, for the active Arrow or Rectangle tool; null for the other tools. */
  captions: boolean | null;
  input: InputView | null;
  setLetter: string | null;
  setCount: number;
  setFull: boolean;
  primary: FinishAction;
  showHint: boolean;
  busy: FinishAction | null;
  notice: Notice | null;
  armed: boolean;
  help: boolean;
  popover: "color" | "width" | null;
  mac: boolean;
}

type EditorSettings = Pick<Settings, "showHints" | "enterAction" | "commentPlacement" | "imageFormat" | "jpgQuality" | "toolPrefs">;

const DEFAULT_SETTINGS: EditorSettings = {
  showHints: true,
  enterAction: "copy",
  commentPlacement: "onImage",
  imageFormat: "png",
  jpgQuality: 90,
  toolPrefs: {},
};

type Drag =
  | { kind: "create"; anchor: Pt; prev: Rect | null }
  | { kind: "moveSel"; start: Pt; orig: Rect }
  | { kind: "resizeSel"; start: Pt; orig: Rect; handle: Handle }
  | { kind: "draw"; start: Pt; ann: Annotation }
  | { kind: "moveAnn"; start: Pt; orig: Annotation; cur: Annotation; moved: boolean }
  | { kind: "editAnn"; start: Pt; orig: Annotation; cur: Annotation; handle: AnnHandle; moved: boolean };

/**
 * How a callout input opened: "draw" right after placing a mark or drawing a captioned shape, "add" for a
 * new caption on a plain shape, "edit" for an existing callout. Null for text.
 */
type InputOrigin = "draw" | "add" | "edit";

interface InputState {
  kind: "mark" | "text";
  id: number;
  value: string;
  /** Text being edited (a working copy; the list keeps the original until the input closes). */
  text: TextAnn | null;
  /** The text is new and not in the list yet. */
  isNew: boolean;
  origin: InputOrigin | null;
  /** For a new callout: the history state right after adding it, to merge the comment into that step. */
  freshState: readonly Annotation[] | null;
}

type CaptionHost = ArrowAnn | RectAnn;

const isCaptionHost = (a: Annotation | null): a is CaptionHost => a?.kind === "arrow" || a?.kind === "rect";

const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** Text of a rejected `invoke`: the core returns the reason as a string. */
function reasonOf(err: unknown): string | undefined {
  const text = typeof err === "string" ? err : err instanceof Error ? err.message : "";
  return text.trim() || undefined;
}

function finishFailedKey(action: FinishAction): string {
  switch (action) {
    case "save":
      return "overlay.notices.saveFailed";
    case "copy":
      return "overlay.notices.copyFailed";
    case "pin":
      return "overlay.notices.pinFailed";
    // Copy all fails before anything is persisted only when adding to the set fails.
    case "addToSet":
    case "copyAll":
      return "overlay.notices.addFailed";
  }
}

/** Browser shortcuts that must not fire inside the overlay (reload, print, find, caret browsing). */
function isBrowserKey(e: KeyboardEvent): boolean {
  if (/^F\d+$/.test(e.key)) return true;
  const mod = isMac ? e.metaKey : e.ctrlKey;
  return mod && !["KeyA", "KeyC", "KeyV", "KeyX", "KeyZ", "KeyY", "ArrowLeft", "ArrowRight", "Backspace", "Delete", "Home", "End"].includes(e.code);
}

export class Editor {
  private readonly ctx: CanvasRenderingContext2D;
  private readonly base = document.createElement("canvas");
  private readonly layer = document.createElement("canvas");
  private layerCtx: CanvasRenderingContext2D | null = null;
  private layerKey: { list: readonly Annotation[]; skip: number | null; letter: string | null } | null = null;
  private measureCtx: OffscreenCanvasRenderingContext2D | null = null;
  private pixels: Uint8ClampedArray | null = null;
  private mosaics = new Map<string, MosaicTile>();

  private payload: PreparePayload | null = null;
  private gen = 0;
  private scale = 1;
  private phase: Phase = "empty";
  private selection: Rect | null = null;
  private corner: Corner = { right: true, bottom: true };
  private tool: Tool = "select";
  private history = new History<readonly Annotation[]>([], 200);
  private nextId = 1;
  private selectedId: number | null = null;
  private drag: Drag | null = null;
  private pointer: Pt | null = null;
  private shift = false;
  private input: InputState | null = null;
  private loupeOn = true;
  private sizeLabelUntil = 0;
  private brushUntil = 0;
  private wheelAcc = 0;
  private lastNudge: { id: number; at: number; state: readonly Annotation[] } | null = null;
  private busy: FinishAction | null = null;
  private notice: Notice | null = null;
  private armed = false;
  private help = false;
  private popover: "color" | "width" | null = null;
  private settings: EditorSettings = DEFAULT_SETTINGS;
  private prefs: ToolPrefs = defaultPrefs();
  private prefsTimer = 0;
  /** Saves of the tool prefs in flight: like the placement, a settings event must not undo them meanwhile. */
  private prefsSaves = 0;
  /** Saves of the placement in flight: until they land, a settings event must not undo the local choice. */
  private placementSaves = 0;
  /** Canvas presses so far, to tell which press drew what. */
  private presses = 0;
  /** The last two shapes drawn and the steps that added them, so a double-click can take back what its own clicks drew. */
  private drawn: { id: number; press: number; before: readonly Annotation[]; after: readonly Annotation[] }[] = [];
  private timers = new Map<string, number>();

  private raf = 0;
  private view: View;
  private listeners = new Set<() => void>();

  constructor(private readonly canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("2d context unavailable");
    this.ctx = ctx;
    this.view = this.buildView();
    canvas.addEventListener("pointerdown", this.onPointerDown);
    canvas.addEventListener("pointermove", this.onPointerMove);
    canvas.addEventListener("pointerup", this.onPointerUp);
    canvas.addEventListener("pointercancel", this.onPointerCancel);
    canvas.addEventListener("pointerleave", this.onPointerLeave);
    canvas.addEventListener("dblclick", this.onDoubleClick);
    canvas.addEventListener("wheel", this.onWheel, { passive: false });
    window.addEventListener("keydown", this.onKeyDown);
    window.addEventListener("keyup", this.onKeyUp);
    window.addEventListener("contextmenu", (e) => {
      if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
    });
    window.addEventListener("resize", () => {
      if (!this.payload) return;
      this.scale = this.computeScale(this.payload);
      this.emit();
      this.requestRender();
    });
  }

  // ---------------------------------------------------------------- store

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getView = (): View => this.view;

  private emit(): void {
    this.view = this.buildView();
    for (const l of this.listeners) l();
  }

  private buildView(): View {
    const s = this.scale;
    const p = this.payload;
    const list = this.history.current;
    return {
      phase: this.phase,
      captureId: p?.captureId ?? null,
      scale: s,
      screen: p ? { w: p.width / s, h: p.height / s } : { w: window.innerWidth, h: window.innerHeight },
      selection: this.selection && scaleRect(this.selection, 1 / s),
      settled: this.phase === "ready" && !(this.drag && (this.drag.kind === "moveSel" || this.drag.kind === "resizeSel" || this.drag.kind === "create")),
      corner: this.corner,
      tool: this.tool,
      context: this.toolContext(),
      canUndo: this.history.canUndo,
      canRedo: this.history.canRedo,
      markCount: this.calloutCount(list),
      commentPlacement: this.settings.commentPlacement,
      captions: this.tool === "arrow" || this.tool === "rect" ? this.prefs.captions[this.tool] : null,
      input: this.inputView(),
      setLetter: p?.setLetter ?? null,
      setCount: p?.setCount ?? 0,
      setFull: p?.setFull ?? false,
      primary: this.primaryAction(),
      showHint: this.settings.showHints,
      busy: this.busy,
      notice: this.notice,
      armed: this.armed,
      help: this.help,
      popover: this.popover,
      mac: isMac,
    };
  }

  /**
   * Callouts as the toolbar counts them. A caption just added to a shape, still empty in its input, is left
   * out: it goes again if it stays empty, and the toolbar must not grow and shrink around every shape.
   */
  private calloutCount(list: readonly Annotation[]): number {
    const inp = this.input;
    const pending = inp && inp.origin !== "edit" && !inp.value.trim() && isCaptionHost(this.find(inp.id)) ? inp.id : null;
    return calloutsOf(list).filter((c) => c.id !== pending).length;
  }

  private toolContext(): ToolContext | null {
    if (this.input?.text) {
      const a = this.input.text;
      return { kind: "text", target: "annotation", color: a.color, size: a.size, mode: null, plate: a.plate };
    }
    const sel = this.selectedAnn() ?? (this.input ? this.find(this.input.id) : null);
    if (sel) {
      return {
        kind: sel.kind,
        target: "annotation",
        color: sel.color,
        size: "size" in sel ? sel.size : null,
        mode: sel.kind === "blur" ? sel.mode : null,
        plate: sel.kind === "text" ? sel.plate : null,
      };
    }
    if (this.tool === "select") return null;
    const pref = this.prefs.tools[this.tool];
    return {
      kind: this.tool,
      target: "tool",
      color: pref.color,
      size: SIZED_TOOLS.includes(this.tool) ? pref.size : null,
      mode: this.tool === "blur" ? this.prefs.blurMode : null,
      plate: this.tool === "text" ? this.prefs.textPlate : null,
    };
  }

  private inputView(): InputView | null {
    const inp = this.input;
    if (!inp) return null;
    const s = this.scale;
    if (inp.text) {
      const l = textLayout({ ...inp.text, text: inp.value || " " }, s, this.measure);
      return {
        kind: "text",
        id: inp.id,
        value: inp.value,
        label: "",
        caption: false,
        stoppable: false,
        anchor: scaleRect(l.box, 1 / s),
        color: inp.text.color,
        plate: inp.text.plate,
        fontPx: l.px / s,
        padX: l.padX / s,
        padY: l.padY / s,
        lineH: l.lineH / s,
        width: l.box.w / s,
      };
    }
    const host = this.find(inp.id);
    const callout = host && calloutOf(host);
    if (!callout) return null;
    const caption = isCaptionHost(host);
    return {
      kind: "mark",
      id: inp.id,
      value: inp.value,
      label: markLabel(this.screenLetter(), callout.num),
      caption,
      stoppable: caption && inp.origin === "draw",
      anchor: scaleRect(markBounds(callout, this.envFor(this.screenLetter())), 1 / s),
      color: callout.color,
      plate: false,
      fontPx: 0,
      padX: 0,
      padY: 0,
      lineH: 0,
      width: 0,
    };
  }

  private primaryAction(): FinishAction {
    const p = this.payload;
    if (p && p.setCount > 0 && !p.setFull) return "copyAll";
    return this.settings.enterAction;
  }

  // ---------------------------------------------------------------- frames and settings

  /** Paints the frozen frame before any other work; must stay synchronous (shared memory is released after it). */
  onFrame(payload: PreparePayload, rgba: Uint8ClampedArray): void {
    const t0 = performance.now();
    this.clearState();
    drawFrame(this.base, rgba, payload.width, payload.height);
    this.canvas.width = payload.width;
    this.canvas.height = payload.height;
    this.ctx.drawImage(this.base, 0, 0);
    this.ctx.fillStyle = DIM;
    this.ctx.fillRect(0, 0, payload.width, payload.height);
    this.reportReady(payload, t0);
    this.load(payload);
  }

  /**
   * Tells the core to show the window once the new frame has been committed, so the first composited frame
   * is never the previous snapshot. A hidden WKWebView throttles rAF, so macOS reports at once; elsewhere the
   * fallback timer keeps a stalled rAF from tripping the core's watchdog.
   */
  private reportReady(payload: PreparePayload, t0: number): void {
    const gen = this.gen;
    let fallback = 0;
    let sent = false;
    const send = () => {
      if (sent || gen !== this.gen) return;
      sent = true;
      window.clearTimeout(fallback);
      api.overlayReady(payload.captureId).catch(() => {});
      const readyMs = performance.now() - t0;
      window.setTimeout(() => {
        api.logMetric(`captureMs(${payload.transport})`, payload.captureMs).catch(() => {});
        api.logMetric("frameToReadyMs", readyMs).catch(() => {});
      }, 0);
    };
    if (isMac) {
      send();
      return;
    }
    fallback = window.setTimeout(send, READY_FALLBACK_MS);
    requestAnimationFrame(() => window.setTimeout(send, 0));
  }

  private load(payload: PreparePayload): void {
    this.payload = payload;
    this.scale = this.computeScale(payload);
    this.phase = "idle";
    this.canvas.style.cursor = "crosshair";
    const gen = this.gen;
    // Read the pixels back once the frame is on screen; the loupe and the mosaic sample from them.
    requestAnimationFrame(() => window.setTimeout(() => gen === this.gen && this.ensurePixels(), 0));
    this.emit();
  }

  /** Drops everything so the next show never flashes a stale frame or state. */
  reset(): void {
    this.clearState();
    // A hidden overlay must not keep a full-monitor backing store (tens of MB of GPU memory per monitor);
    // the next frame resizes the canvas again before the window is shown.
    this.canvas.width = 1;
    this.canvas.height = 1;
    this.ctx.fillStyle = "#000";
    this.ctx.fillRect(0, 0, 1, 1);
    this.emit();
  }

  private clearState(): void {
    this.flushPrefs();
    this.gen++;
    for (const id of this.timers.values()) window.clearTimeout(id);
    this.timers.clear();
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.payload = null;
    this.phase = "empty";
    this.selection = null;
    this.corner = { right: true, bottom: true };
    this.tool = "select";
    this.history.reset([]);
    this.selectedId = null;
    this.drag = null;
    this.pointer = null;
    this.input = null;
    this.drawn = [];
    this.sizeLabelUntil = 0;
    this.brushUntil = 0;
    this.lastNudge = null;
    this.busy = null;
    this.notice = null;
    this.armed = false;
    this.help = false;
    this.popover = null;
    this.pixels = null;
    this.mosaics.clear();
    this.layerKey = null;
    this.layer.width = 1;
    this.layer.height = 1;
    this.layerCtx = null;
    this.base.width = 1;
    this.base.height = 1;
  }

  applySettings(s: Settings): void {
    this.settings = this.placementSaves > 0 ? { ...s, commentPlacement: this.settings.commentPlacement } : s;
    if (!this.prefsTimer && this.prefsSaves === 0) this.prefs = parsePrefs(s.toolPrefs);
    this.emit();
    this.requestRender();
  }

  /** Comments on the image or as separate text; applies at once and is remembered for the next snapshots. */
  setCommentPlacement(placement: CommentPlacement): void {
    if (this.busy || placement === this.settings.commentPlacement) return;
    this.settings = { ...this.settings, commentPlacement: placement };
    this.placementSaves++;
    api
      .updateSettings({ commentPlacement: placement })
      .catch(() => {})
      .finally(() => this.placementSaves--);
    this.emit();
    this.requestRender();
  }

  private savePrefs(): void {
    window.clearTimeout(this.prefsTimer);
    this.prefsTimer = window.setTimeout(() => this.flushPrefs(), PREFS_SAVE_MS);
  }

  private flushPrefs(): void {
    if (!this.prefsTimer) return;
    window.clearTimeout(this.prefsTimer);
    this.prefsTimer = 0;
    this.prefsSaves++;
    api
      .updateSettings({ toolPrefs: serializePrefs(this.prefs) })
      .catch(() => {})
      .finally(() => this.prefsSaves--);
  }

  // ---------------------------------------------------------------- helpers

  /**
   * Physical px per CSS px. Normally devicePixelRatio; if it disagrees with how the frame actually maps onto
   * the window (DPI change while hidden), the layout ratio wins, since that is what the pointer sees.
   */
  private computeScale(p: PreparePayload): number {
    const dpr = window.devicePixelRatio || p.monitor.scale;
    const css = window.innerWidth;
    if (css <= 0 || Math.abs(p.width / dpr - css) <= 2) return dpr;
    return p.width / css;
  }

  private measure: Measure = (text, font) => {
    if (!this.measureCtx) this.measureCtx = new OffscreenCanvas(1, 1).getContext("2d");
    if (!this.measureCtx) return text.length * 8;
    this.measureCtx.font = font;
    return this.measureCtx.measureText(text).width;
  };

  private screenLetter(): string | null {
    return this.payload?.setLetter ?? null;
  }

  private envFor(letter: string | null): RenderEnv {
    return { scale: this.scale, letter, measure: this.measure, mosaic: (a) => this.mosaicTile(a) };
  }

  private bounds(): Size {
    return { w: this.payload?.width ?? 0, h: this.payload?.height ?? 0 };
  }

  private find(id: number): Annotation | null {
    return this.history.current.find((a) => a.id === id) ?? null;
  }

  private selectedAnn(): Annotation | null {
    if (this.selectedId === null) return null;
    const d = this.drag;
    if (d && (d.kind === "moveAnn" || d.kind === "editAnn") && d.orig.id === this.selectedId) return d.cur;
    return this.find(this.selectedId);
  }

  private toPhys(e: { clientX: number; clientY: number }): Pt {
    return { x: e.clientX * this.scale, y: e.clientY * this.scale };
  }

  private later(name: string, ms: number, fn: () => void): void {
    window.clearTimeout(this.timers.get(name));
    this.timers.set(
      name,
      window.setTimeout(() => {
        this.timers.delete(name);
        fn();
      }, ms),
    );
  }

  private showNotice(n: Notice, ms: number): void {
    this.notice = n;
    this.later("notice", ms, () => {
      this.notice = null;
      this.emit();
    });
  }

  private ensurePixels(): Uint8ClampedArray | null {
    const p = this.payload;
    if (!p) return null;
    if (!this.pixels) {
      const ctx = this.base.getContext("2d");
      if (!ctx) return null;
      this.pixels = ctx.getImageData(0, 0, p.width, p.height).data;
    }
    return this.pixels;
  }

  private pixelSource(px: Uint8ClampedArray, width: number, height: number): PixelSource {
    return {
      width,
      height,
      read(x, y, out) {
        const i = (y * width + x) * 4;
        out[0] = px[i];
        out[1] = px[i + 1];
        out[2] = px[i + 2];
      },
    };
  }

  private mosaicTile(a: BlurAnn): MosaicTile | null {
    const px = this.ensurePixels();
    const p = this.payload;
    if (!px || !p) return null;
    const cell = mosaicCellSize(a.r, this.scale);
    const exclude = this.history.current.filter((b): b is BlurAnn => b.kind === "blur" && b.id !== a.id).map((b) => b.r);
    const key = [a.r, ...exclude].map((r) => `${r.x},${r.y},${r.w},${r.h}`).join(";") + `|${cell}`;
    const cached = this.mosaics.get(key);
    if (cached) return cached;
    const grid = computeMosaic(this.pixelSource(px, p.width, p.height), a.r, cell, exclude);
    const image = new OffscreenCanvas(grid.cols, grid.rows);
    image.getContext("2d")?.putImageData(new ImageData(grid.data, grid.cols, grid.rows), 0, 0);
    const tile = { image, cols: grid.cols, rows: grid.rows, cell };
    if (this.mosaics.size >= MOSAIC_CACHE) this.mosaics.delete(this.mosaics.keys().next().value as string);
    this.mosaics.set(key, tile);
    return tile;
  }

  /** The pixel is hidden by a committed blur (whole pixels, as drawn). */
  private underBlur(x: number, y: number): boolean {
    return this.history.current.some(
      (a) => a.kind === "blur" && x >= Math.floor(a.r.x) && x < Math.ceil(a.r.x + a.r.w) && y >= Math.floor(a.r.y) && y < Math.ceil(a.r.y + a.r.h),
    );
  }

  private pixelHex(x: number, y: number): string {
    const p = this.payload;
    const px = this.ensurePixels();
    if (!p || !px) return "#000000";
    const i = (y * p.width + x) * 4;
    return "#" + [px[i], px[i + 1], px[i + 2]].map((v) => v.toString(16).padStart(2, "0")).join("").toUpperCase();
  }

  // ---------------------------------------------------------------- rendering

  requestRender(): void {
    if (!this.raf && this.payload) this.raf = requestAnimationFrame(this.render);
  }

  private skipId(): number | null {
    const d = this.drag;
    if (d && (d.kind === "moveAnn" || d.kind === "editAnn")) return d.orig.id;
    if (this.input?.text && !this.input.isNew) return this.input.id;
    return null;
  }

  private liveAnnotation(): Annotation | null {
    const d = this.drag;
    if (!d) return null;
    if (d.kind === "draw") return d.ann;
    if (d.kind === "moveAnn" || d.kind === "editAnn") return d.cur;
    return null;
  }

  /** Frozen frame plus every committed annotation except `skip`; rebuilt only when that changes. */
  private layerFor(skip: number | null): CanvasImageSource {
    const list = this.history.current;
    if (!list.some((a) => a.id !== skip)) return this.base;
    const letter = this.screenLetter();
    const k = this.layerKey;
    if (k && k.list === list && k.skip === skip && k.letter === letter) return this.layer;
    const p = this.payload!;
    if (this.layer.width !== p.width || this.layer.height !== p.height || !this.layerCtx) {
      this.layer.width = p.width;
      this.layer.height = p.height;
      this.layerCtx = this.layer.getContext("2d", { alpha: false });
    }
    const ctx = this.layerCtx!;
    ctx.drawImage(this.base, 0, 0);
    drawAnnotations(ctx, list, this.envFor(letter), skip);
    this.layerKey = { list, skip, letter };
    return this.layer;
  }

  private render = (): void => {
    this.raf = 0;
    const p = this.payload;
    if (!p || this.phase === "empty") return;
    const ctx = this.ctx;
    const W = p.width;
    const H = p.height;
    ctx.save();
    ctx.drawImage(this.layerFor(this.skipId()), 0, 0);
    const live = this.liveAnnotation();
    if (live) drawAnnotation(ctx, live, this.envFor(this.screenLetter()));

    const sel = this.selection;
    if (sel && sel.w >= 1 && sel.h >= 1) {
      const list = live ? this.history.current.map((a) => (a.id === live.id ? live : a)) : this.history.current;
      drawCallouts(ctx, list, this.envFor(this.screenLetter()), {
        // Separate text is still shown, as a ghost, so the user sees what goes along with the image.
        style: this.settings.commentPlacement === "onImage" ? "solid" : "ghost",
        crop: sel,
        hideId: this.input?.kind === "mark" ? this.input.id : null,
      });
    }
    ctx.fillStyle = DIM;
    if (!sel || sel.w < 1 || sel.h < 1) {
      ctx.fillRect(0, 0, W, H);
    } else {
      ctx.fillRect(0, 0, W, sel.y);
      ctx.fillRect(0, sel.y + sel.h, W, H - sel.y - sel.h);
      ctx.fillRect(0, sel.y, sel.x, sel.h);
      ctx.fillRect(sel.x + sel.w, sel.y, W - sel.x - sel.w, sel.h);
      this.drawSelectionFrame(ctx, sel);
    }
    this.drawAnnotationChrome(ctx);
    if (sel) this.drawSizeLabel(ctx, sel);
    this.drawBrushPreview(ctx);
    this.drawLoupe(ctx);
    ctx.restore();
  };

  private lineWidth(): number {
    return Math.max(1, Math.round(this.scale));
  }

  private drawSelectionFrame(ctx: CanvasRenderingContext2D, sel: Rect): void {
    const lw = this.lineWidth();
    ctx.lineWidth = lw;
    ctx.strokeStyle = "#FFFFFF";
    ctx.strokeRect(sel.x - lw * 1.5, sel.y - lw * 1.5, sel.w + lw * 3, sel.h + lw * 3);
    ctx.strokeStyle = ACCENT;
    ctx.strokeRect(sel.x - lw / 2, sel.y - lw / 2, sel.w + lw, sel.h + lw);
    if (this.drag?.kind === "create") return;
    const size = Math.round(HANDLE_CSS * this.scale);
    for (const h of visibleHandles(sel, HIT_CSS * this.scale)) {
      const q = handlePoint(sel, h);
      const x = Math.round(q.x - size / 2);
      const y = Math.round(q.y - size / 2);
      ctx.fillStyle = "#FFFFFF";
      ctx.fillRect(x, y, size, size);
      ctx.strokeStyle = ACCENT;
      ctx.strokeRect(x + lw / 2, y + lw / 2, size - lw, size - lw);
    }
  }

  private drawAnnotationChrome(ctx: CanvasRenderingContext2D): void {
    const a = this.selectedAnn();
    if (!a) return;
    const s = this.scale;
    const lw = this.lineWidth();
    const env = this.envFor(this.screenLetter());
    if (a.kind !== "rect" && a.kind !== "blur") {
      const b = inflate(bounds(a, env), 4 * s);
      ctx.setLineDash([4 * s, 3 * s]);
      ctx.lineWidth = lw;
      ctx.strokeStyle = "#FFFFFF";
      ctx.strokeRect(b.x, b.y, b.w, b.h);
      ctx.lineDashOffset = 4 * s;
      ctx.strokeStyle = ACCENT;
      ctx.strokeRect(b.x, b.y, b.w, b.h);
      ctx.setLineDash([]);
      ctx.lineDashOffset = 0;
    } else {
      ctx.lineWidth = lw;
      ctx.strokeStyle = ACCENT;
      ctx.strokeRect(a.r.x, a.r.y, a.r.w, a.r.h);
    }
    for (const h of annotationHandles(a, HIT_CSS * s)) {
      ctx.beginPath();
      ctx.arc(h.p.x, h.p.y, 4.5 * s, 0, Math.PI * 2);
      ctx.fillStyle = "#FFFFFF";
      ctx.fill();
      ctx.lineWidth = lw;
      ctx.strokeStyle = ACCENT;
      ctx.stroke();
    }
  }

  private drawPlate(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, font: string): { w: number; h: number } {
    const s = this.scale;
    ctx.font = font;
    const w = Math.round(ctx.measureText(text).width + 12 * s);
    const h = Math.round(22 * s);
    ctx.fillStyle = GLASS;
    ctx.beginPath();
    ctx.roundRect(x, y, w, h, 5 * s);
    ctx.fill();
    ctx.fillStyle = "#F4F6FA";
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.fillText(text, x + 6 * s, y + h / 2 + 0.5 * s);
    return { w, h };
  }

  private drawSizeLabel(ctx: CanvasRenderingContext2D, sel: Rect): void {
    const d = this.drag;
    const active = d && (d.kind === "create" || d.kind === "moveSel" || d.kind === "resizeSel");
    if (!active && performance.now() > this.sizeLabelUntil) return;
    if (sel.w < 1 || sel.h < 1) return;
    const s = this.scale;
    const h = Math.round(22 * s);
    const gap = Math.round(6 * s);
    const outside = sel.y - h - gap >= 0;
    const x = Math.round(outside ? sel.x : sel.x + 4 * s);
    const y = Math.round(outside ? sel.y - h - gap : sel.y + 4 * s);
    this.drawPlate(ctx, `${Math.round(sel.w)} x ${Math.round(sel.h)}`, x, y, `600 ${12 * s}px ${UI_FONT}`);
  }

  private touchSizeLabel(): void {
    this.sizeLabelUntil = performance.now() + SIZE_LABEL_MS;
    this.later("sizeLabel", SIZE_LABEL_MS + 20, () => this.requestRender());
  }

  private strokeWidth(): number | null {
    const c = this.toolContext();
    if (!c || c.size === null) return null;
    if (c.kind === "marker") return markerWidth(c.size, this.scale);
    if (c.kind === "text") return null;
    return c.size * this.scale;
  }

  private drawBrushPreview(ctx: CanvasRenderingContext2D): void {
    if (!this.pointer || performance.now() > this.brushUntil) return;
    const w = this.strokeWidth();
    if (!w) return;
    ctx.beginPath();
    ctx.arc(this.pointer.x, this.pointer.y, w / 2, 0, Math.PI * 2);
    ctx.lineWidth = this.lineWidth();
    ctx.strokeStyle = "#000000";
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(this.pointer.x, this.pointer.y, w / 2 + this.lineWidth(), 0, Math.PI * 2);
    ctx.strokeStyle = "#FFFFFF";
    ctx.stroke();
  }

  private loupeVisible(): boolean {
    if (!this.loupeOn || !this.pointer || this.input || this.busy) return false;
    const d = this.drag;
    return this.phase === "idle" || d?.kind === "create" || d?.kind === "resizeSel";
  }

  private drawLoupe(ctx: CanvasRenderingContext2D): void {
    const p = this.payload;
    if (!p || !this.pointer || !this.loupeVisible()) return;
    const s = this.scale;
    const cell = Math.max(1, Math.round(LOUPE_CELL_CSS * s));
    const size = cell * LOUPE_PIXELS;
    const labelH = Math.round(22 * s);
    const gap = Math.round(6 * s);
    const cx = Math.min(p.width - 1, Math.max(0, Math.floor(this.pointer.x)));
    const cy = Math.min(p.height - 1, Math.max(0, Math.floor(this.pointer.y)));
    const pos = placeLoupe(this.pointer, { w: size, h: size + gap + labelH }, this.bounds(), Math.round(24 * s));
    const x = Math.round(pos.x);
    const y = Math.round(pos.y);
    const half = (LOUPE_PIXELS - 1) / 2;
    const radius = 8 * s;

    ctx.save();
    ctx.beginPath();
    ctx.roundRect(x, y, size, size, radius);
    ctx.clip();
    ctx.fillStyle = "#000000";
    ctx.fillRect(x, y, size, size);
    ctx.imageSmoothingEnabled = false;
    // The rendered layer, not the raw frame: a blur must stay a blur under the magnifier too.
    ctx.drawImage(this.layerFor(this.skipId()), cx - half, cy - half, LOUPE_PIXELS, LOUPE_PIXELS, x, y, size, size);
    ctx.fillStyle = "rgba(255, 255, 255, 0.07)";
    for (let i = 1; i < LOUPE_PIXELS; i++) {
      ctx.fillRect(x + i * cell, y, 1, size);
      ctx.fillRect(x, y + i * cell, size, 1);
    }
    const lw = this.lineWidth();
    const c0x = x + half * cell;
    const c0y = y + half * cell;
    ctx.lineWidth = lw;
    ctx.strokeStyle = "#000000";
    ctx.strokeRect(c0x - lw / 2, c0y - lw / 2, cell + lw, cell + lw);
    ctx.strokeStyle = "#FFFFFF";
    ctx.strokeRect(c0x - lw * 1.5, c0y - lw * 1.5, cell + lw * 3, cell + lw * 3);
    ctx.restore();

    ctx.beginPath();
    ctx.roundRect(x + lw / 2, y + lw / 2, size - lw, size - lw, radius);
    ctx.lineWidth = lw;
    ctx.strokeStyle = "rgba(255, 255, 255, 0.55)";
    ctx.stroke();

    const font = `600 ${12 * s}px ${UI_FONT}`;
    if (this.underBlur(cx, cy)) {
      this.drawPlate(ctx, `${cx}, ${cy}`, x, y + size + gap, font);
      return;
    }
    const hex = this.pixelHex(cx, cy);
    const plate = this.drawPlate(ctx, `${cx}, ${cy}   ${hex}`, x, y + size + gap, font);
    const sw = Math.round(10 * s);
    ctx.fillStyle = hex;
    ctx.fillRect(x + plate.w + gap, y + size + gap + (labelH - sw) / 2, sw, sw);
    ctx.lineWidth = lw;
    ctx.strokeStyle = "#FFFFFF";
    ctx.strokeRect(x + plate.w + gap, y + size + gap + (labelH - sw) / 2, sw, sw);
  }

  // ---------------------------------------------------------------- pointer

  private onPointerDown = (e: PointerEvent): void => {
    this.presses++;
    if (!this.payload || this.phase === "empty" || this.busy || this.drag || e.button !== 0) return;
    e.preventDefault();
    // preventDefault also keeps the focus where it was: a toolbar button reached with Tab would keep it and
    // take the next Enter natively (Close drops the snapshot without the Esc confirmation).
    if (document.activeElement instanceof HTMLButtonElement) document.activeElement.blur();
    const p = this.toPhys(e);
    this.pointer = p;
    this.shift = e.shiftKey;
    this.armed = false;
    this.popover = null;
    this.help = false;
    // A click outside an open input commits it. A caption opened by drawing lets the press through, so the
    // next shape starts at once (drag, type, drag); every other input only commits.
    const through = !this.input || (this.input.origin === "draw" && isCaptionHost(this.find(this.input.id)));
    if (this.input) this.closeInput();
    if (through && this.beginDrag(p)) this.canvas.setPointerCapture(e.pointerId);
    this.updateHover(p);
    this.emit();
    this.requestRender();
  };

  private beginDrag(p: Pt): boolean {
    const sel = this.selection;
    if (this.phase === "idle" || !sel) return this.startCreate(p, null);
    const s = this.scale;
    const tol = HIT_CSS * s;
    const list = this.history.current;
    const env = this.envFor(this.screenLetter());
    const selHandle = hitHandle(sel, p, tol);

    if (this.tool === "select") {
      const current = this.selectedAnn();
      const annHandle = current && hitAnnotationHandle(current, p, tol, env);
      if (current && annHandle) {
        this.drag = { kind: "editAnn", start: p, orig: current, cur: current, handle: annHandle, moved: false };
        return true;
      }
      const hit = topHit(list, p, env, tol / 2);
      if (hit) {
        this.selectedId = hit.id;
        this.drag = { kind: "moveAnn", start: p, orig: hit, cur: hit, moved: false };
        return true;
      }
      this.selectedId = null;
      if (selHandle) {
        this.drag = { kind: "resizeSel", start: p, orig: sel, handle: selHandle };
        return true;
      }
      if (contains(sel, p)) {
        this.drag = { kind: "moveSel", start: p, orig: sel };
        return true;
      }
      return list.length === 0 ? this.startCreate(p, sel) : false;
    }

    this.selectedId = null;
    if (selHandle) {
      this.drag = { kind: "resizeSel", start: p, orig: sel, handle: selHandle };
      return true;
    }
    if (!contains(sel, p) && list.length === 0) return this.startCreate(p, sel);

    const tool = this.tool;
    if (tool === "mark") {
      // A press on a callout badge (a mark or a caption) moves that callout instead of stacking a new mark.
      const hit = topCallout(list, p, env, tol / 2);
      if (hit) {
        this.selectedId = hit.id;
        this.drag = { kind: "moveAnn", start: p, orig: hit, cur: hit, moved: false };
        return true;
      }
      // A callout outside the selection would be in neither the image nor the text.
      if (!contains(sel, p)) return false;
      if (calloutsOf(list).length >= MAX_MARKS) {
        this.noticeMarksLimit();
        return false;
      }
    }
    if (tool === "text") {
      const hit = topHit(list, p, env, 0, ["text"]);
      if (hit && hit.kind === "text") this.openTextInput(hit, false);
      else this.openNewText(p);
      return false;
    }
    this.drag = { kind: "draw", start: p, ann: this.newAnnotation(tool, p) };
    return true;
  }

  private startCreate(p: Pt, prev: Rect | null): boolean {
    const q = clampPt({ x: Math.round(p.x), y: Math.round(p.y) }, this.bounds());
    this.drag = { kind: "create", anchor: q, prev };
    this.selection = { x: q.x, y: q.y, w: 0, h: 0 };
    this.phase = "creating";
    this.selectedId = null;
    return true;
  }

  private newAnnotation(tool: DrawTool, p: Pt): Annotation {
    const id = this.nextId++;
    const { color, size } = this.prefs.tools[tool];
    switch (tool) {
      case "arrow":
      case "marker":
        return { kind: tool, id, color, size, a: p, b: p };
      case "pencil":
        return { kind: "pencil", id, color, size, points: [p] };
      case "rect":
        return { kind: "rect", id, color, size, r: { x: p.x, y: p.y, w: 0, h: 0 } };
      case "blur":
        return { kind: "blur", id, color, mode: this.prefs.blurMode, r: { x: Math.round(p.x), y: Math.round(p.y), w: 0, h: 0 } };
      case "mark":
        return { kind: "mark", id, color, num: nextMarkNum(this.history.current), p, tail: null, comment: "" };
      case "text":
        return { kind: "text", id, color, size, p, text: "", plate: this.prefs.textPlate };
    }
  }

  private onPointerMove = (e: PointerEvent): void => {
    if (!this.payload || this.phase === "empty") return;
    const p = this.toPhys(e);
    this.pointer = p;
    this.shift = e.shiftKey;
    const d = this.drag;
    if (d) {
      if (d.kind === "draw" && d.ann.kind === "pencil") {
        const events = typeof e.getCoalescedEvents === "function" ? e.getCoalescedEvents() : [];
        for (const ev of events.length ? events : [e]) this.addPencilPoint(d.ann.points, this.toPhys(ev));
      } else {
        this.updateDrag(p);
      }
      this.requestRender();
      return;
    }
    this.updateHover(p);
    if (this.loupeVisible() || performance.now() < this.brushUntil) this.requestRender();
  };

  private addPencilPoint(points: Pt[], p: Pt): void {
    const last = points[points.length - 1];
    if (Math.hypot(p.x - last.x, p.y - last.y) >= 1.5 * this.scale) points.push(p);
  }

  private updateDrag(p: Pt): void {
    const d = this.drag;
    if (!d) return;
    const bounds = this.bounds();
    const dx = p.x - ("start" in d ? d.start.x : 0);
    const dy = p.y - ("start" in d ? d.start.y : 0);
    // Blur boxes stay on whole pixels, like the area they protect; this also keeps the mosaic cache keys stable.
    const blur = (d.kind === "moveAnn" || d.kind === "editAnn") && d.orig.kind === "blur";
    const whole = (v: number) => (blur ? Math.round(v) : v);
    switch (d.kind) {
      case "create": {
        let end = clampPt({ x: Math.round(p.x), y: Math.round(p.y) }, bounds);
        if (this.shift) end = clampPt(squareEnd(d.anchor, end), bounds);
        this.selection = normalize(d.anchor, end);
        break;
      }
      case "moveSel":
        this.selection = moveRect(d.orig, Math.round(dx), Math.round(dy), bounds);
        break;
      case "resizeSel":
        this.selection = resizeRect(d.orig, d.handle, Math.round(dx), Math.round(dy), bounds);
        break;
      case "draw":
        d.ann = this.updateDraft(d.ann, d.start, p);
        break;
      case "moveAnn":
        // A click that selects must not nudge the object by a pixel of hand jitter.
        if (!d.moved && Math.hypot(dx, dy) < CLICK_SLOP_CSS * this.scale) break;
        d.moved = true;
        d.cur = translate(d.orig, whole(dx), whole(dy));
        break;
      case "editAnn":
        // Same slop as a move: a click on a handle (the caption badge of a selected shape is one) is no edit.
        if (!d.moved && Math.hypot(dx, dy) < CLICK_SLOP_CSS * this.scale) break;
        d.moved = true;
        d.cur = dragHandle(d.orig, d.handle, whole(dx), whole(dy), this.shift);
        break;
    }
  }

  private updateDraft(a: Annotation, start: Pt, p: Pt): Annotation {
    switch (a.kind) {
      case "arrow":
        return { ...a, b: this.shift ? snap45(a.a, p) : p };
      case "marker":
        return { ...a, b: this.shift ? snapAxis(a.a, p) : p };
      case "rect":
        return { ...a, r: normalize(start, this.shift ? squareEnd(start, p) : p) };
      case "blur": {
        const end = clampPt({ x: Math.round(p.x), y: Math.round(p.y) }, this.bounds());
        return { ...a, r: normalize({ x: Math.round(start.x), y: Math.round(start.y) }, end) };
      }
      case "mark":
        return { ...a, tail: Math.hypot(p.x - a.p.x, p.y - a.p.y) > TAIL_MIN_CSS * this.scale ? p : null };
      default:
        return a;
    }
  }

  private onPointerUp = (e: PointerEvent): void => {
    const d = this.drag;
    if (!d) return;
    if (this.canvas.hasPointerCapture(e.pointerId)) this.canvas.releasePointerCapture(e.pointerId);
    // The release point can differ from the last move event; apply it before committing.
    const end = this.toPhys(e);
    this.pointer = end;
    this.shift = e.shiftKey;
    if (d.kind === "draw" && d.ann.kind === "pencil") this.addPencilPoint(d.ann.points, end);
    else this.updateDrag(end);
    this.drag = null;
    switch (d.kind) {
      case "create": {
        const sel = this.selection;
        if (sel && sel.w >= MIN_SELECTION && sel.h >= MIN_SELECTION) {
          this.phase = "ready";
          this.corner = nearestCorner(sel, end);
          this.touchSizeLabel();
        } else if (d.prev) {
          this.selection = d.prev;
          this.phase = "ready";
        } else {
          this.selection = null;
          this.phase = "idle";
        }
        break;
      }
      case "moveSel":
      case "resizeSel":
        if (!this.selection || this.selection.w < MIN_SELECTION || this.selection.h < MIN_SELECTION) {
          this.selection = d.orig;
        } else if (d.kind === "resizeSel") {
          // Only the axes the handle moved: after the right edge the pointer is mid-height, which says nothing.
          const near = nearestCorner(this.selection, end);
          this.corner = {
            right: /[ew]/.test(d.handle) ? near.right : this.corner.right,
            bottom: /[ns]/.test(d.handle) ? near.bottom : this.corner.bottom,
          };
        }
        this.touchSizeLabel();
        break;
      case "draw":
        this.commitDraft(d.ann);
        break;
      case "moveAnn":
      case "editAnn":
        if (d.cur !== d.orig && this.isValid(d.cur)) this.replaceAnnotation(d.cur);
        break;
    }
    if (d.kind === "create" || d.kind === "moveSel" || d.kind === "resizeSel" || d.kind === "moveAnn" || d.kind === "editAnn") this.noticeMarksOut();
    this.updateHover(this.toPhys(e));
    this.emit();
    this.requestRender();
  };

  private onPointerCancel = (): void => {
    if (this.drag) this.cancelDrag();
    this.emit();
    this.requestRender();
  };

  private onPointerLeave = (): void => {
    if (this.drag) return;
    this.pointer = null;
    this.requestRender();
  };

  private cancelDrag(): void {
    const d = this.drag;
    if (!d) return;
    this.drag = null;
    if (d.kind === "create") {
      this.selection = d.prev;
      this.phase = d.prev ? "ready" : "idle";
    } else if (d.kind === "moveSel" || d.kind === "resizeSel") {
      this.selection = d.orig;
    }
  }

  private onDoubleClick = (e: MouseEvent): void => {
    if (this.phase !== "ready" || this.busy || !["select", "mark", "arrow", "rect"].includes(this.tool)) return;
    const p = this.toPhys(e);
    const env = this.envFor(this.screenLetter());
    const tol = (HIT_CSS * this.scale) / 2;
    // The shape tools caption shapes; marks and text open with the Select and Callout tools, as before.
    const kinds: AnnotationKind[] = this.tool === "arrow" || this.tool === "rect" ? ["arrow", "rect"] : ["mark", "text", "arrow", "rect"];
    const list = this.history.current;
    const strays = this.doubleClickStrays();
    let hit = topHit(list.filter((a) => !strays.includes(a.id)), p, env, tol, kinds);
    // The double-click aimed at `hit`: what its clicks drew on the way (the Callout tool's mark, a shaky
    // shape) goes without a trace. With nothing else there, the Callout tool edits the mark it just placed.
    if (hit) strays.forEach(() => this.history.discard());
    else if (strays.length) hit = topHit(list.filter((a) => strays.includes(a.id)), p, env, tol, ["mark"]);
    if (!hit) return;
    if (hit.kind === "text") this.openTextInput(hit, false);
    else if (calloutOf(hit)) this.openCalloutInput(hit.id, "edit");
    else if (isCaptionHost(hit)) this.addCaption(hit);
    else return;
    this.emit();
    this.requestRender();
  };

  private onWheel = (e: WheelEvent): void => {
    e.preventDefault();
    if (this.phase !== "ready" || this.busy || this.tool === "select" || !SIZED_TOOLS.includes(this.tool)) return;
    this.wheelAcc += e.deltaY;
    if (Math.abs(this.wheelAcc) < WHEEL_STEP) return;
    const dir = this.wheelAcc < 0 ? 1 : -1;
    this.wheelAcc = 0;
    this.stepSize(dir);
  };

  private updateHover(p: Pt): void {
    const cursor = this.cursorAt(p);
    if (this.canvas.style.cursor !== cursor) this.canvas.style.cursor = cursor;
  }

  private cursorAt(p: Pt): string {
    const d = this.drag;
    if (d) {
      if (d.kind === "moveSel" || d.kind === "moveAnn") return "move";
      if (d.kind === "resizeSel") return cursorForHandle(d.handle);
      return this.canvas.style.cursor || "crosshair";
    }
    const sel = this.selection;
    if (this.phase !== "ready" || !sel) return "crosshair";
    const tol = HIT_CSS * this.scale;
    const env = this.envFor(this.screenLetter());
    const list = this.history.current;
    const current = this.selectedAnn();
    if (this.tool === "select") {
      if (current && hitAnnotationHandle(current, p, tol, env)) return "pointer";
      if (topHit(list, p, env, tol / 2)) return "move";
    }
    const h = hitHandle(sel, p, tol);
    if (h) return cursorForHandle(h);
    if (this.tool === "select") return contains(sel, p) ? "move" : list.length ? "default" : "crosshair";
    if (this.tool === "mark" && topCallout(list, p, env, tol / 2)) return "move";
    if (this.tool === "text") return "text";
    return "crosshair";
  }

  // ---------------------------------------------------------------- annotations

  private isValid(a: Annotation): boolean {
    const min = 3 * this.scale;
    switch (a.kind) {
      case "arrow":
      case "marker":
        return Math.hypot(a.b.x - a.a.x, a.b.y - a.a.y) >= min;
      case "rect":
      case "blur":
        return a.r.w >= min && a.r.h >= min;
      default:
        return true;
    }
  }

  private commitDraft(a: Annotation): void {
    if (!this.isValid(a)) return;
    const list = this.history.current;
    if (isCaptionHost(a) && this.prefs.captions[a.kind] && this.captionFits(a) && this.captionAllowed(a)) {
      // One step for the shape and its caption: one Undo removes both.
      this.history.push([...list, { ...a, caption: { num: nextMarkNum(list), comment: "" } }]);
      this.noteDrawn(a.id, list);
      this.openCalloutInput(a.id, "draw");
      return;
    }
    this.history.push([...list, a]);
    this.noteDrawn(a.id, list);
    if (a.kind === "mark") this.openCalloutInput(a.id, "draw");
  }

  private noteDrawn(id: number, before: readonly Annotation[]): void {
    this.drawn = [...this.drawn.slice(-1), { id, press: this.presses, before, after: this.history.current }];
  }

  /**
   * Ids of what the two presses of the current double-click drew, newest first, while those steps are still
   * the latest ones: the Callout tool's first click places a mark, a shaky click of a shape tool draws a
   * tiny shape. A shape drawn by any earlier press is meant and stays.
   */
  private doubleClickStrays(): number[] {
    const ids: number[] = [];
    let state = this.history.current;
    for (let i = this.drawn.length - 1; i >= 0; i--) {
      const d = this.drawn[i];
      if (d.press < this.presses - 1 || d.after !== state) break;
      ids.push(d.id);
      state = d.before;
    }
    return ids;
  }

  /** A shape shorter than the badge would hide under it: it stays plain, silently. */
  private captionFits(a: CaptionHost): boolean {
    const len = a.kind === "arrow" ? Math.hypot(a.b.x - a.a.x, a.b.y - a.a.y) : Math.max(a.r.w, a.r.h);
    return len >= BADGE_HEIGHT * this.scale;
  }

  /** Whether a caption may be added to `a`; says why not otherwise. */
  private captionAllowed(a: CaptionHost): boolean {
    const sel = this.selection;
    const anchor = a.kind === "arrow" ? a.a : { x: a.r.x, y: a.r.y };
    // Like a mark: a badge outside the selection would be in neither the image nor the text.
    if (!sel || !contains(sel, anchor)) {
      this.showNotice({ key: "overlay.notices.captionOutside", tone: "info" }, 3000);
      return false;
    }
    if (calloutsOf(this.history.current).length >= MAX_MARKS) {
      this.noticeMarksLimit();
      return false;
    }
    return true;
  }

  private noticeMarksLimit(): void {
    this.showNotice({ key: "overlay.notices.marksLimit", params: { count: MAX_MARKS }, tone: "error" }, 3000);
  }

  /** Double-click on a plain arrow or rectangle: a caption with the next number, left without a trace if it stays empty. */
  private addCaption(a: CaptionHost): void {
    if (!this.captionAllowed(a)) return;
    this.history.push(setCaption(this.history.current, a.id, ""));
    this.openCalloutInput(a.id, "add");
  }

  private replaceAnnotation(next: Annotation): void {
    this.history.push(this.history.current.map((a) => (a.id === next.id ? next : a)));
  }

  private deleteSelected(): void {
    if (this.selectedId === null) return;
    this.history.push(removeAnnotation(this.history.current, this.selectedId));
    this.selectedId = null;
  }

  private openCalloutInput(id: number, origin: InputOrigin): void {
    const host = this.find(id);
    const callout = host && calloutOf(host);
    if (!callout) return;
    this.input = {
      kind: "mark",
      id,
      value: callout.comment,
      text: null,
      isNew: false,
      origin,
      freshState: origin === "edit" ? null : this.history.current,
    };
    this.selectedId = null;
  }

  private openTextInput(a: TextAnn, isNew: boolean): void {
    this.input = { kind: "text", id: a.id, value: a.text, text: { ...a }, isNew, origin: null, freshState: null };
    this.selectedId = null;
  }

  private openNewText(p: Pt): void {
    const draft = this.newAnnotation("text", p) as TextAnn;
    // Put the click point at the vertical middle of the first line, where the caret appears.
    const l = textLayout({ ...draft, text: "" }, this.scale, this.measure);
    this.openTextInput({ ...draft, p: { x: p.x - l.padX, y: p.y - l.padY - l.lineH / 2 } }, true);
  }

  setInputValue(value: string): void {
    if (!this.input) return;
    this.input.value = value;
    this.emit();
  }

  /** Commits the open input; with `id`, only if that input is still the open one (a late blur of a closed one). */
  closeInput(id?: number): void {
    const inp = this.input;
    if (!inp || (id !== undefined && inp.id !== id)) return;
    this.input = null;
    const list = this.history.current;
    if (inp.kind === "mark") {
      const host = this.find(inp.id);
      const comment = inp.value.replace(/\s+/g, " ").trim();
      // Merge into the step that added the callout while nothing came in between (a style change does).
      const fresh = inp.freshState === list;
      const commit = (next: Annotation[]) => (fresh ? this.history.replace(next) : this.history.push(next));
      if (host?.kind === "mark") {
        if (comment !== host.comment) commit(list.map((a) => (a.id === inp.id ? { ...host, comment } : a)));
      } else if (isCaptionHost(host) && host.caption) {
        // An empty caption is no caption: it leaves the plain shape and frees its number.
        if (comment) {
          if (comment !== host.caption.comment) commit(setCaption(list, inp.id, comment));
        } else if (inp.origin === "add" && fresh) {
          this.history.discard();
        } else {
          commit(setCaption(list, inp.id, null));
        }
      }
    } else if (inp.text) {
      const text = inp.value.replace(/\s+/g, " ").trim();
      const ann: TextAnn = { ...inp.text, text };
      if (inp.isNew) {
        if (text) this.history.push([...list, ann]);
      } else if (!text) {
        this.history.push(removeAnnotation(list, inp.id));
      } else {
        const orig = this.find(inp.id);
        if (orig && JSON.stringify(orig) !== JSON.stringify(ann)) this.replaceAnnotation(ann);
      }
    }
    this.emit();
    this.requestRender();
  }

  /** Enter in the comment input: commit and stay ready for the next mark. */
  submitInput(): void {
    this.closeInput();
  }

  // ---------------------------------------------------------------- toolbar actions

  setTool(tool: Tool): void {
    if (this.phase !== "ready" || this.busy) return;
    if (this.input) this.closeInput();
    this.tool = tool;
    this.selectedId = null;
    this.popover = null;
    this.armed = false;
    if (this.pointer) this.updateHover(this.pointer);
    this.emit();
    this.requestRender();
  }

  /** Caption after drawing, on or off for the active Arrow or Rectangle tool; remembered with the tool prefs. */
  toggleCaptions(): void {
    if (this.phase !== "ready" || this.busy || (this.tool !== "arrow" && this.tool !== "rect")) return;
    this.prefs.captions[this.tool] = !this.prefs.captions[this.tool];
    this.savePrefs();
    this.emit();
  }

  /** "Turn off" in a caption input opened by drawing: no caption after the next shapes; this one keeps its text. */
  stopCaptions(): void {
    const host = this.input && this.find(this.input.id);
    if (this.busy || !isCaptionHost(host)) return;
    this.prefs.captions[host.kind] = false;
    this.savePrefs();
    this.closeInput();
  }

  togglePopover(which: "color" | "width"): void {
    this.popover = this.popover === which ? null : which;
    this.emit();
  }

  closePopover(): void {
    if (!this.popover) return;
    this.popover = null;
    this.emit();
  }

  /** Applies `change` to the annotation the toolbar edits, or to the active tool's defaults. */
  private applyStyle(change: (a: Annotation) => Annotation | null, prefsChange: (p: ToolPrefs, kind: AnnotationKind) => void): void {
    const c = this.toolContext();
    if (!c) return;
    prefsChange(this.prefs, c.kind);
    this.savePrefs();
    if (c.target === "annotation") {
      if (this.input?.text) {
        const next = change(this.input.text);
        if (next) this.input.text = next as TextAnn;
      } else {
        const target = this.selectedAnn() ?? (this.input ? this.find(this.input.id) : null);
        const next = target && change(target);
        if (target && next && JSON.stringify(next) !== JSON.stringify(target)) this.replaceAnnotation(next);
      }
    }
    this.emit();
    this.requestRender();
  }

  setColor(color: string): void {
    this.applyStyle(
      (a) => ({ ...a, color }),
      (p, kind) => {
        p.tools[kind].color = color;
      },
    );
    this.popover = null;
    this.emit();
  }

  setSize(size: number): void {
    this.applyStyle(
      (a) => ("size" in a ? { ...a, size } : null),
      (p, kind) => {
        if (SIZED_TOOLS.includes(kind)) p.tools[kind].size = size;
      },
    );
    this.popover = null;
    this.brushUntil = performance.now() + BRUSH_PREVIEW_MS;
    this.later("brush", BRUSH_PREVIEW_MS + 20, () => this.requestRender());
    this.emit();
  }

  private stepSize(dir: 1 | -1): void {
    const c = this.toolContext();
    if (!c || c.size === null) return;
    const next = stepWidth(c.size, dir);
    if (next !== c.size) this.setSize(next);
    else {
      this.brushUntil = performance.now() + BRUSH_PREVIEW_MS;
      this.requestRender();
    }
  }

  toggleBlurMode(): void {
    const c = this.toolContext();
    if (!c || c.kind !== "blur") return;
    const mode: BlurMode = c.mode === "solid" ? "mosaic" : "solid";
    this.applyStyle(
      (a) => (a.kind === "blur" ? { ...a, mode } : null),
      (p) => {
        p.blurMode = mode;
      },
    );
  }

  setTextPlate(plate: boolean): void {
    this.applyStyle(
      (a) => (a.kind === "text" ? { ...a, plate } : null),
      (p) => {
        p.textPlate = plate;
      },
    );
  }

  undo(): void {
    if (this.drag || this.busy) return;
    if (this.input) this.closeInput();
    if (this.history.undo()) this.afterHistoryChange();
  }

  redo(): void {
    if (this.drag || this.busy) return;
    if (this.input) this.closeInput();
    if (this.history.redo()) this.afterHistoryChange();
  }

  private afterHistoryChange(): void {
    if (this.selectedId !== null && !this.find(this.selectedId)) this.selectedId = null;
    this.lastNudge = null;
    this.noticeMarksOut();
    this.emit();
    this.requestRender();
  }

  /** Callouts left outside the selection go into neither the image nor the text: say so instead of dropping them silently. */
  private noticeMarksOut(): void {
    const sel = this.selection;
    if (!sel || this.phase !== "ready") return;
    const count = calloutsOf(this.history.current).filter((c) => !markInCrop(c, sel)).length;
    if (count > 0) this.showNotice({ key: "overlay.notices.marksOut", params: { count }, tone: "info" }, 4000);
    else if (this.notice?.key === "overlay.notices.marksOut") this.notice = null;
  }

  toggleHelp(): void {
    this.help = !this.help;
    this.popover = null;
    this.emit();
  }

  cancel(): void {
    if (this.busy) return;
    api.overlayCancel().catch(() => {});
  }

  async finish(action: FinishAction, hint?: FinishHint): Promise<void> {
    const payload = this.payload;
    const crop = this.selection;
    if (this.busy || this.phase !== "ready" || !payload || !crop) return;
    if ((action === "addToSet" || action === "copyAll") && payload.setFull) return;
    if (this.input) this.closeInput();
    if (this.drag) this.cancelDrag();
    const gen = this.gen;
    this.busy = action;
    this.notice = null;
    this.popover = null;
    this.help = false;
    this.armed = false;
    this.emit();
    this.requestRender();
    let exported = false;
    try {
      const letter = action === "copy" || action === "save" || action === "pin" ? payload.setLetter : (payload.setLetter ?? "A");
      const format: ImageFormat = action === "save" ? this.settings.imageFormat : "png";
      const list = this.history.current;
      // A pin has nowhere to put separate text, so its comments are always drawn on it.
      const placement = action === "pin" ? "onImage" : this.settings.commentPlacement;
      this.ensurePixels();
      const result = await exportImage({
        frame: this.base,
        crop,
        annotations: list,
        env: this.envFor(letter),
        commentsOnImage: placement === "onImage",
        format,
        quality: this.settings.jpgQuality / 100,
      });
      if (gen !== this.gen) return;
      exported = true;
      await api.overlayFinish(
        result.bytes,
        finishMeta({ captureId: payload.captureId, action, letter, format, placement, annotations: list, crop, image: result, scale: this.scale, hint }),
      );
    } catch (err) {
      // On success the core hides and resets the overlay. On failure it leaves it open with the selection,
      // the annotations and the frame intact, so the user can retry or pick another action.
      if (gen !== this.gen) return;
      this.busy = null;
      if (exported) this.showNotice({ key: finishFailedKey(action), detail: reasonOf(err), tone: "error" }, 8000);
      else this.showNotice({ key: "overlay.notices.finishFailed", tone: "error" }, 6000);
      this.emit();
      this.requestRender();
    }
  }

  // ---------------------------------------------------------------- keyboard

  private onKeyDown = (e: KeyboardEvent): void => {
    if (!this.payload || this.phase === "empty") return;
    const target = e.target as HTMLElement | null;
    if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target?.isContentEditable) {
      if (isBrowserKey(e)) e.preventDefault();
      return;
    }
    if (e.key === "Shift" && !this.shift) {
      this.shift = true;
      if (this.drag && this.pointer) {
        this.updateDrag(this.pointer);
        this.requestRender();
      }
    }
    // A focused toolbar button activates natively on Enter and Space.
    if (target instanceof HTMLButtonElement && ["Enter", "NumpadEnter", "Space"].includes(e.code)) return;
    if (e.code !== "Tab") e.preventDefault();
    const cmd = matchKey(e, isMac);
    if (this.busy) return;
    if (!cmd) {
      const habit = matchHabit(e, isMac);
      // One-shot like Esc and copy: a held Ctrl+X must not walk on to discarding the snapshot.
      if (habit && !e.repeat) this.runHabit(habit);
      return;
    }
    // A held key must not walk through one-shot commands: held Esc would discard, held Enter would finish.
    if (e.repeat && !isRepeatable(cmd)) return;
    this.run(cmd, e.code);
  };

  private onKeyUp = (e: KeyboardEvent): void => {
    if (e.key !== "Shift" || !this.shift) return;
    this.shift = false;
    if (this.drag && this.pointer) {
      this.updateDrag(this.pointer);
      this.requestRender();
    }
  };

  private run(cmd: CommandId, code: string): void {
    if (cmd !== "escape" && this.armed) this.armed = false;
    const ready = this.phase === "ready";
    if (cmd.startsWith("tool.")) {
      if (ready) this.setTool(cmd.slice(5) as Tool);
      return;
    }
    switch (cmd) {
      case "primary": {
        if (!ready) return;
        const sel = this.selectedAnn();
        if (sel && calloutOf(sel)) this.openCalloutInput(sel.id, "edit");
        else if (sel?.kind === "text") this.openTextInput(sel, false);
        else {
          void this.finish(this.primaryAction());
          return;
        }
        break;
      }
      case "addToSet":
      case "copy":
      case "save":
      case "pin":
        void this.finish(cmd);
        return;
      case "undo":
        this.undo();
        return;
      case "redo":
        this.redo();
        return;
      case "selectAll": {
        if (this.drag) return;
        const b = this.bounds();
        this.selection = { x: 0, y: 0, w: b.w, h: b.h };
        this.phase = "ready";
        this.touchSizeLabel();
        break;
      }
      case "delete":
        if (!ready) return;
        this.deleteSelected();
        break;
      case "move":
      case "moveFast":
      case "resize":
      case "resizeFast":
        if (!ready || this.drag) return;
        this.nudge(arrowDelta(code), cmd === "moveFast" || cmd === "resizeFast" ? 10 : 1, cmd.startsWith("resize"));
        break;
      case "widthDown":
      case "widthUp":
        if (ready) this.stepSize(cmd === "widthUp" ? 1 : -1);
        return;
      case "loupe":
        this.loupeOn = !this.loupeOn;
        break;
      case "commentPlacement":
        // Same condition as the toolbar switch: only meaningful once there is a callout.
        if (ready && calloutsOf(this.history.current).length > 0) {
          this.setCommentPlacement(this.settings.commentPlacement === "onImage" ? "separate" : "onImage");
        }
        return;
      case "captions":
        this.toggleCaptions();
        return;
      case "pickColor":
        if (this.phase === "idle" && this.pointer) this.copyHex(this.pointer);
        return;
      case "help":
        this.toggleHelp();
        return;
      case "escape":
        this.escape();
        return;
    }
    this.emit();
    this.requestRender();
  }

  /** Lightshot's keys: nothing is uploaded, so "upload" copies (the toast says why) and the others explain or close. */
  private runHabit(habit: HabitId): void {
    if (habit === "close") {
      this.escape();
      return;
    }
    this.armed = false;
    if (habit === "upload" && this.phase === "ready") {
      void this.finish("copy", "noUpload");
      return;
    }
    const notice: Notice =
      habit === "upload"
        ? {
            key: "overlay.notices.noUpload",
            params: { copy: shortcutOf("copy", isMac), paste: formatChord({ code: "KeyV", mod: true }, isMac) },
            tone: "info",
          }
        : { key: "overlay.notices.editHere", tone: "info" };
    this.showNotice(notice, 6000);
    this.emit();
  }

  private escape(): void {
    const step = escapeStep({
      inputOpen: !!this.input,
      panelOpen: !!this.popover || this.help,
      dragging: !!this.drag,
      tool: this.tool,
      annotationSelected: this.selectedId !== null,
      hasAnnotations: this.history.current.length > 0,
      armed: this.armed,
    });
    switch (step) {
      case "closeInput":
        this.closeInput();
        break;
      case "closePanel":
        this.popover = null;
        this.help = false;
        break;
      case "cancelDrag":
        this.cancelDrag();
        break;
      case "leaveTool":
        this.setTool("select");
        break;
      case "deselect":
        this.selectedId = null;
        break;
      case "arm":
        this.armed = true;
        this.later("arm", ARM_MS, () => {
          this.armed = false;
          this.emit();
        });
        break;
      case "cancel":
        this.cancel();
        return;
    }
    this.emit();
    this.requestRender();
  }

  private nudge(d: Pt, step: number, resize: boolean): void {
    const sel = this.selection;
    if (!sel) return;
    const selected = this.selectedAnn();
    if (selected && !resize) {
      const next = translate(selected, d.x * step, d.y * step);
      const list = this.history.current.map((a) => (a.id === next.id ? next : a));
      const now = performance.now();
      const merge = this.lastNudge && this.lastNudge.id === next.id && now - this.lastNudge.at < NUDGE_MERGE_MS && this.lastNudge.state === this.history.current;
      if (merge) this.history.replace(list);
      else this.history.push(list);
      this.lastNudge = { id: next.id, at: now, state: this.history.current };
      if (calloutOf(next)) this.noticeMarksOut();
      return;
    }
    const bounds = this.bounds();
    if (resize) {
      const next = resizeRect(sel, "se", d.x * step, d.y * step, bounds);
      if (next.w >= MIN_SELECTION && next.h >= MIN_SELECTION && next.x === sel.x && next.y === sel.y) this.selection = next;
    } else {
      this.selection = moveRect(sel, d.x * step, d.y * step, bounds);
    }
    this.touchSizeLabel();
    this.noticeMarksOut();
  }

  private copyHex(p: Pt): void {
    const b = this.bounds();
    const x = Math.min(b.w - 1, Math.max(0, Math.floor(p.x)));
    const y = Math.min(b.h - 1, Math.max(0, Math.floor(p.y)));
    if (this.underBlur(x, y)) {
      this.showNotice({ key: "overlay.notices.hexHidden", tone: "info" }, 2500);
      this.emit();
      return;
    }
    const hex = this.pixelHex(x, y);
    navigator.clipboard.writeText(hex).then(
      () => {
        this.showNotice({ key: "overlay.notices.hexCopied", params: { hex }, tone: "info" }, 1800);
        this.emit();
      },
      () => {
        this.showNotice({ key: "overlay.notices.hexFailed", tone: "error" }, 3000);
        this.emit();
      },
    );
  }
}
