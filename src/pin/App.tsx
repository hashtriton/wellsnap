// A pinned screenshot: the image fills its own borderless window (src-tauri/src/pins.rs). Drag moves it,
// the wheel zooms around the cursor, the opacity modifier plus wheel fades it, a double-click folds it to a
// small preview, the right button opens the menu.
import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent, type PointerEvent, type WheelEvent } from "react";
import { useI18n } from "../shared/i18n";
import { api, pinUrl, type PinInfo } from "../shared/ipc";
import { guessPlatform } from "../shared/ui/hotkey";
import { Icon } from "../shared/ui/icons";
import { percent, repeatStep, stepOpacity, stepZoom, windowSize, zoomOf } from "./view";

/** A press has to travel this far (css px) before it moves the window, so clicks and double-clicks stay clicks. */
const DRAG_THRESHOLD = 3;
const BADGE_MS = 1200;
const OPACITY_PRESETS = [1, 0.75, 0.5, 0.25];
/** Wheel distance (px) per zoom or opacity step: one mouse notch, while a trackpad's many small events add up. */
const WHEEL_STEP = 100;

export function App() {
  const { t } = useI18n();
  const mac = useMemo(() => guessPlatform(navigator.userAgent) === "macos", []);
  const [info, setInfo] = useState<PinInfo | null>(null);
  const [zoom, setZoom] = useState(1);
  const [opacity, setOpacity] = useState(1);
  /** The system fades the window itself (macOS), so the image stays opaque. */
  const [nativeFade, setNativeFade] = useState(false);
  const [folded, setFolded] = useState(false);
  const [badge, setBadge] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const badgeTimer = useRef<number | undefined>(undefined);
  const press = useRef<{ x: number; y: number } | null>(null);
  const wheel = useRef(0);
  const lastMenu = useRef<Menu | null>(null);
  /** The view as of the last change: wheel events and key repeats come faster than React renders. */
  const cur = useRef({ zoom: 1, opacity: 1, folded: false });

  useEffect(() => {
    document.title = t("pin.title");
  }, [t]);

  useEffect(() => {
    api.pinInfo().then(setInfo).catch(() => undefined);
    // Asked once up front, so the first fade is not applied twice (by the image and by the window).
    api.pinOpacity(1).then(setNativeFade).catch(() => undefined);
    return () => {
      window.clearTimeout(badgeTimer.current);
      void lastMenu.current?.close();
    };
  }, []);

  // Moved onto a monitor with another scale, the system resized the window: read the zoom back from it.
  useEffect(() => {
    if (!info) return;
    const unlisten = getCurrentWindow().onScaleChanged(({ payload }) => {
      if (cur.current.folded) return;
      cur.current.zoom = zoomOf(info, payload.size.width);
      setZoom(cur.current.zoom);
    });
    return () => void unlisten.then((f) => f());
  }, [info]);

  const flash = useCallback((text: string) => {
    setBadge(text);
    window.clearTimeout(badgeTimer.current);
    badgeTimer.current = window.setTimeout(() => setBadge(null), BADGE_MS);
  }, []);

  const resize = useCallback(
    (nextZoom: number, nextFolded: boolean, anchor: [number, number]) => {
      if (!info) return;
      const size = windowSize(info, nextZoom, nextFolded, window.devicePixelRatio || 1);
      cur.current.zoom = nextZoom;
      cur.current.folded = nextFolded;
      setZoom(nextZoom);
      setFolded(nextFolded);
      api.pinResize(size.width, size.height, anchor[0], anchor[1]).catch(() => undefined);
    },
    [info],
  );

  const zoomTo = useCallback(
    (next: number, anchor: [number, number] = [0.5, 0.5]) => {
      if (cur.current.folded || next === cur.current.zoom) return;
      resize(next, false, anchor);
      flash(t("pin.zoom", { value: percent(next) }));
    },
    [resize, flash, t],
  );

  const fadeTo = useCallback(
    (next: number) => {
      cur.current.opacity = next;
      setOpacity(next);
      api.pinOpacity(next).catch(() => undefined);
      flash(t("pin.opacityValue", { value: percent(next) }));
    },
    [flash, t],
  );

  const reset = useCallback(() => {
    const { opacity, folded, zoom } = cur.current;
    if (opacity !== 1) fadeTo(1);
    if (folded || zoom !== 1) {
      resize(1, false, [0, 0]);
      flash(t("pin.zoom", { value: percent(1) }));
    }
  }, [fadeTo, resize, flash, t]);

  const run = useCallback((action: () => Promise<unknown>) => {
    setFailed(false);
    action().catch(() => {
      setFailed(true);
      window.setTimeout(() => setFailed(false), 3000);
    });
  }, []);

  const copy = useCallback(() => run(api.pinCopy), [run]);
  const save = useCallback(() => run(api.pinSave), [run]);
  const close = useCallback(() => void api.pinClose(), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = mac ? e.metaKey : e.ctrlKey;
      if (e.code === "Escape") close();
      else if (mod && e.code === "KeyC") copy();
      else if (mod && e.code === "KeyS") save();
      else if (e.code === "Digit0" || e.code === "Numpad0") reset();
      else if (["Equal", "NumpadAdd"].includes(e.code)) {
        if (mod) fadeTo(stepOpacity(cur.current.opacity, 1));
        else zoomTo(stepZoom(cur.current.zoom, 1));
      } else if (["Minus", "NumpadSubtract"].includes(e.code)) {
        if (mod) fadeTo(stepOpacity(cur.current.opacity, -1));
        else zoomTo(stepZoom(cur.current.zoom, -1));
      } else if (e.code.startsWith("Arrow")) {
        const step = e.shiftKey ? 10 : 1;
        const dx = e.code === "ArrowLeft" ? -step : e.code === "ArrowRight" ? step : 0;
        const dy = e.code === "ArrowUp" ? -step : e.code === "ArrowDown" ? step : 0;
        void api.pinNudge(dx, dy);
      } else if (!/^F\d+$/.test(e.key) && !mod) return;
      // Also the browser's own keys: F5 or Ctrl+R would reload the page and lose the zoom, Ctrl+P would print.
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mac, close, copy, save, reset, fadeTo, zoomTo]);

  const onWheel = (e: WheelEvent) => {
    if (e.deltaY === 0) return;
    const delta = e.deltaY * (e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1);
    // A turn the other way starts over. A fast spin may arrive as one event of several notches.
    wheel.current = Math.sign(wheel.current) === Math.sign(delta) ? wheel.current + delta : delta;
    const steps = Math.trunc(Math.abs(wheel.current) / WHEEL_STEP);
    if (steps === 0) return;
    wheel.current -= Math.sign(wheel.current) * steps * WHEEL_STEP;
    const dir = delta < 0 ? 1 : -1;
    // Option on macOS: Ctrl+wheel there is often a trackpad pinch.
    const fade = mac ? e.altKey : e.ctrlKey;
    if (fade) {
      fadeTo(repeatStep(cur.current.opacity, steps, (v) => stepOpacity(v, dir)));
      return;
    }
    zoomTo(repeatStep(cur.current.zoom, steps, (v) => stepZoom(v, dir)), [e.clientX / window.innerWidth, e.clientY / window.innerHeight]);
  };

  const onPointerDown = (e: PointerEvent) => {
    if (e.button === 1) {
      e.preventDefault();
      reset();
      return;
    }
    press.current = e.button === 0 && !(e.target as HTMLElement).closest("button") ? { x: e.clientX, y: e.clientY } : null;
  };

  const onPointerMove = (e: PointerEvent) => {
    const start = press.current;
    if (!start || (e.buttons & 1) === 0) {
      press.current = null;
      return;
    }
    if (Math.hypot(e.clientX - start.x, e.clientY - start.y) < DRAG_THRESHOLD) return;
    press.current = null;
    void getCurrentWindow().startDragging();
  };

  const onDoubleClick = (e: MouseEvent) => {
    if ((e.target as HTMLElement).closest("button")) return;
    resize(cur.current.zoom, !cur.current.folded, [e.clientX / window.innerWidth, e.clientY / window.innerHeight]);
  };

  const onContextMenu = async (e: MouseEvent) => {
    e.preventDefault();
    const accel = (key: string) => (mac ? `Cmd+${key}` : `Ctrl+${key}`);
    void lastMenu.current?.close();
    const menu = await Menu.new({
      items: [
        { text: t("pin.copy"), accelerator: accel("C"), action: copy },
        { text: t("pin.save"), accelerator: accel("S"), action: save },
        { item: "Separator" },
        { text: t("pin.actualSize"), accelerator: "0", enabled: folded || zoom !== 1 || opacity !== 1, action: reset },
        {
          text: t("pin.opacity"),
          items: OPACITY_PRESETS.map((value) => ({ text: percent(value), checked: Math.abs(value - opacity) < 0.01, action: () => fadeTo(value) })),
        },
        { item: "Separator" },
        { text: t("pin.close"), accelerator: "Escape", action: close },
        { text: t("pin.closeAll"), action: () => void api.pinCloseAll() },
      ],
    });
    lastMenu.current = menu;
    await menu.popup();
  };

  if (!info) return null;
  const dpr = window.devicePixelRatio || 1;
  const size = windowSize(info, zoom, folded, dpr);
  // The buttons need room; a folded preview is only for moving and unfolding.
  const tooSmall = folded || size.width / dpr < 96 || size.height / dpr < 48;
  return (
    <main
      className="pin"
      title={t("pin.hint", { mod: mac ? "⌥" : "Ctrl" })}
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onDoubleClick={onDoubleClick}
      onContextMenu={(e) => void onContextMenu(e)}
    >
      <img
        src={pinUrl(info.id)}
        alt={t("pin.title")}
        draggable={false}
        style={nativeFade ? undefined : { opacity }}
        onLoad={() => void api.pinShow()}
        onError={() => void api.pinShow()}
      />
      {!tooSmall && (
        <div className="pin-tools">
          <button type="button" aria-label={t("pin.copy")} title={t("pin.copy")} onClick={copy}>
            <Icon name="copy" size={14} />
          </button>
          <button type="button" aria-label={t("pin.close")} title={t("pin.close")} onClick={close}>
            <Icon name="x" size={14} />
          </button>
        </div>
      )}
      {(badge || failed) && !folded && (
        <div className={`pin-badge${failed ? " pin-badge-error" : ""}`} role="status">
          {failed ? t("pin.failed") : badge}
        </div>
      )}
    </main>
  );
}
