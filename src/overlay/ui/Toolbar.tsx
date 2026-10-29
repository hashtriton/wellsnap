import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import type { TFunction } from "../../shared/i18n";
import type { CommentPlacement, FinishAction } from "../../shared/ipc";
import type { Editor, View } from "../editor";
import { placeToolbar, type Side, type ToolbarPlacement } from "../geometry";
import { shortcutOf, TOOL_COMMANDS, type CommandId } from "../keys";
import { MARK_COLOR, PALETTE, WIDTHS } from "../prefs";
import type { Size, Tool } from "../types";
import { Floating, rectOf } from "./Floating";
import {
  ArrowIcon,
  BlurIcon,
  CaptionIcon,
  CloseIcon,
  CopyIcon,
  PinIcon,
  ImageTextIcon,
  MarkerIcon,
  MarkIcon,
  PencilIcon,
  PlateIcon,
  PlusIcon,
  RectIcon,
  RedoIcon,
  SaveIcon,
  SelectIcon,
  SolidIcon,
  TextIcon,
  TextLinesIcon,
  UndoIcon,
  WidthIcon,
} from "./Icons";

type Orientation = ToolbarPlacement["orientation"];

const TOOL_ICONS: Record<Tool, () => ReactNode> = {
  select: SelectIcon,
  arrow: ArrowIcon,
  pencil: PencilIcon,
  rect: RectIcon,
  marker: MarkerIcon,
  blur: BlurIcon,
  mark: MarkIcon,
  text: TextIcon,
};
const DRAWING: Tool[] = ["arrow", "pencil", "rect", "marker", "blur", "mark", "text"];
/** Stroke weight of the width preview in icon units for S/M/L. */
const WIDTH_WEIGHT: Record<number, number> = { 2: 1.75, 4: 3.25, 8: 5.5 };
const TOOLTIP_DELAY = 350;

/** Keeps focus where it is (canvas or an open input) when a toolbar button is clicked with the mouse. */
const keepFocus = (e: ReactMouseEvent) => e.preventDefault();

interface Tip {
  id: string;
  text: string;
  sub?: string;
}

interface Props {
  editor: Editor;
  view: View;
  t: TFunction;
}

export function Toolbar({ editor, view, t }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const buttons = useRef<Record<string, HTMLButtonElement | null>>({});
  const sizes = useRef<Partial<Record<Orientation, Size>>>({});
  /** Center of the primary button along each bar, so it lands where the selection drag ended. */
  const primaryAt = useRef<Partial<Record<Orientation, number>>>({});
  const [orientation, setOrientation] = useState<Orientation>("horizontal");
  /** Icon-only buttons when the labeled bar is wider than the monitor (very narrow or highly scaled screens). */
  const [compact, setCompact] = useState(false);
  const labeledWidth = useRef(0);
  const [place, setPlace] = useState<ToolbarPlacement | null>(null);
  const [tip, setTip] = useState<Tip | null>(null);
  const tipTimer = useRef(0);
  const sel = view.selection!;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    const room = view.screen.w - 16;
    if (orientation === "horizontal" && !compact) labeledWidth.current = box.width;
    if (compact !== labeledWidth.current > room) {
      setCompact(labeledWidth.current > room);
      return;
    }
    sizes.current[orientation] = { w: box.width, h: box.height };
    const btn = buttons.current[view.primary]?.getBoundingClientRect();
    if (orientation === "horizontal") primaryAt.current.horizontal = btn ? btn.left + btn.width / 2 - box.left : box.width;
    else primaryAt.current.vertical = btn ? btn.top + btn.height / 2 - box.top : box.height;
    const horiz = sizes.current.horizontal ?? { w: box.width, h: box.height };
    // Before the vertical bar was measured, estimate it from the horizontal one (labels become icons).
    const vert = sizes.current.vertical ?? { w: horiz.h, h: horiz.w * 0.8 };
    const h = primaryAt.current.horizontal ?? horiz.w;
    const v = primaryAt.current.vertical ?? (h / horiz.w) * vert.h;
    const next = placeToolbar(sel, horiz, vert, view.screen, { corner: view.corner, h, v });
    if (next.orientation !== orientation) {
      setOrientation(next.orientation);
      return;
    }
    if (!place || place.x !== next.x || place.y !== next.y || place.side !== next.side) setPlace(next);
  });

  useEffect(() => () => window.clearTimeout(tipTimer.current), []);

  const vertical = orientation === "vertical";
  const side: Side[] = (() => {
    switch (place?.side) {
      case "above":
        return ["above", "below"];
      case "right":
        return ["right", "left"];
      case "left":
        return ["left", "right"];
      case "inside":
        return ["above", "below"];
      default:
        return ["below", "above"];
    }
  })();

  const showTip = (next: Tip) => {
    window.clearTimeout(tipTimer.current);
    tipTimer.current = window.setTimeout(() => setTip(next), tip ? 0 : TOOLTIP_DELAY);
  };
  const hideTip = () => {
    window.clearTimeout(tipTimer.current);
    setTip(null);
  };
  // A shown tooltip follows its button's text, which can change under the pointer (Shift+T flips captions).
  const retip = (next: Tip) => setTip((cur) => (cur?.id === next.id && (cur.text !== next.text || cur.sub !== next.sub) ? next : cur));

  const withKey = (label: string, cmd: CommandId) => {
    const key = shortcutOf(cmd, view.mac);
    return key ? t("overlay.withKey", { label, key }) : label;
  };

  const busy = view.busy !== null;
  const ctx = view.context;

  const common = {
    iconOnly: vertical || compact,
    popover: view.popover,
    register: (id: string, el: HTMLButtonElement | null) => {
      buttons.current[id] = el;
    },
    onTip: showTip,
    onUntip: hideTip,
    onRetip: retip,
  };

  const spinnerOr = (action: FinishAction, icon: ReactNode) => (view.busy === action ? <span className="spinner" aria-hidden="true" /> : icon);

  const toolButton = (tool: Tool) => {
    const Icon = TOOL_ICONS[tool];
    const label = t(`overlay.tools.${tool}`);
    return (
      <ToolButton {...common}
        key={tool}
        id={`tool-${tool}`}
        label={label}
        tip={withKey(tool === "mark" ? t("overlay.markTip") : label, TOOL_COMMANDS[tool])}
        pressed={view.tool === tool}
        disabled={busy}
        onClick={() => editor.setTool(tool)}
      >
        <Icon />
      </ToolButton>
    );
  };

  const colorDisabled = busy || !ctx || (ctx.kind === "blur" && ctx.mode !== "solid");
  const widthDisabled = busy || !ctx || ctx.size === null;
  const setOpen = view.setCount > 0 && !view.setFull;

  const optionSlot =
    ctx?.kind === "blur" ? (
      <ToolButton {...common}
        id="blurMode"
        label={t(ctx.mode === "solid" ? "overlay.blurMode.solid" : "overlay.blurMode.mosaic")}
        sub={`${t("overlay.blurMode.toggle", {
          mode: t(ctx.mode === "solid" ? "overlay.blurMode.mosaicName" : "overlay.blurMode.solidName"),
        })}. ${t("overlay.blurMode.advice")}`}
        pressed={ctx.mode === "solid"}
        disabled={busy}
        onClick={() => editor.toggleBlurMode()}
      >
        {ctx.mode === "solid" ? <SolidIcon /> : <BlurIcon />}
      </ToolButton>
    ) : (
      <ToolButton {...common}
        id="width"
        label={widthDisabled ? t("overlay.widthDisabled") : t("overlay.width")}
        tip={widthDisabled ? t("overlay.widthDisabled") : `${t("overlay.width")} ([ ])`}
        disabled={widthDisabled}
        haspopup
        onClick={() => editor.togglePopover("width")}
      >
        <WidthIcon weight={WIDTH_WEIGHT[ctx?.size ?? 4] ?? 3.25} />
      </ToolButton>
    );

  const placementButton = (placement: CommentPlacement, label: string, icon: ReactNode) => (
    <ToolButton {...common}
      id={`placement-${placement}`}
      label={label}
      tip={withKey(label, "commentPlacement")}
      pressed={view.commentPlacement === placement}
      disabled={busy}
      onClick={() => editor.setCommentPlacement(placement)}
    >
      {icon}
    </ToolButton>
  );

  const hint = [
    view.commentPlacement === "separate" ? t("overlay.placement.note") : null,
    view.setLetter === null && !view.setFull ? t("overlay.setLettersHint") : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const tipAnchor = tip ? () => rectOf(buttons.current[tip.id]) : null;
  const popAnchor = view.popover ? () => rectOf(buttons.current[view.popover!]) : null;
  const swatches = ctx?.kind === "mark" ? [MARK_COLOR, ...PALETTE] : [...PALETTE];

  return (
    <>
      <div
        ref={ref}
        role="toolbar"
        aria-label={t("overlay.toolbar")}
        aria-orientation={orientation}
        aria-busy={busy}
        className={`toolbar glass${vertical ? " vertical" : ""}`}
        style={{ left: place?.x ?? 0, top: place?.y ?? 0, visibility: place ? "visible" : "hidden" }}
      >
        {toolButton("select")}
        <span className="tb-sep" />
        {DRAWING.map(toolButton)}
        <span className="tb-sep" />
        <ToolButton {...common}
          id="color"
          label={colorDisabled ? t("overlay.colorDisabled") : t("overlay.color")}
          disabled={colorDisabled}
          haspopup
          onClick={() => editor.togglePopover("color")}
        >
          <span className="swatch-dot" style={{ background: colorDisabled ? "transparent" : ctx?.color }} />
        </ToolButton>
        {optionSlot}
        {view.captions !== null && (
          <ToolButton {...common}
            id="captions"
            label={t("overlay.captions.label")}
            tip={withKey(t("overlay.captions.label"), "captions")}
            sub={view.captions ? t(`overlay.captions.on.${view.tool}`) : t("overlay.captions.off")}
            pressed={view.captions}
            disabled={busy}
            onClick={() => editor.toggleCaptions()}
          >
            <CaptionIcon />
          </ToolButton>
        )}
        {view.markCount > 0 && (
          <>
            <span className="tb-sep" />
            <div className="tb-seg" role="group" aria-label={t("overlay.placement.label")}>
              {placementButton("onImage", t("overlay.placement.onImage"), <ImageTextIcon />)}
              {placementButton("separate", t("overlay.placement.separate"), <TextLinesIcon />)}
            </div>
          </>
        )}
        <span className="tb-sep" />
        <ToolButton {...common} id="undo" label={t("overlay.undo")} tip={withKey(t("overlay.undo"), "undo")} disabled={busy || !view.canUndo} onClick={() => editor.undo()}>
          <UndoIcon />
        </ToolButton>
        <ToolButton {...common} id="redo" label={t("overlay.redo")} tip={withKey(t("overlay.redo"), "redo")} disabled={busy || !view.canRedo} onClick={() => editor.redo()}>
          <RedoIcon />
        </ToolButton>
        <span className="tb-sep" />
        <ToolButton {...common}
          id="addToSet"
          label={t("overlay.addToSet")}
          tip={view.setFull ? t("overlay.setFull") : withKey(t("overlay.addToSetHint"), "addToSet")}
          sub={!view.setFull && view.setLetter === null ? t("overlay.setLettersHint") : undefined}
          text={t("overlay.addToSet")}
          disabled={busy || view.setFull}
          onClick={() => void editor.finish("addToSet")}
        >
          {spinnerOr("addToSet", <PlusIcon />)}
        </ToolButton>
        <ToolButton {...common}
          id="pin"
          label={t("overlay.pin")}
          tip={withKey(t("overlay.pinHint"), "pin")}
          disabled={busy}
          onClick={() => void editor.finish("pin")}
        >
          {spinnerOr("pin", <PinIcon />)}
        </ToolButton>
        <ToolButton {...common}
          id="save"
          label={t("overlay.save")}
          tip={withKey(t("overlay.save"), view.primary === "save" ? "primary" : "save")}
          primary={view.primary === "save"}
          disabled={busy}
          onClick={() => void editor.finish("save")}
        >
          {spinnerOr("save", <SaveIcon />)}
        </ToolButton>
        <ToolButton {...common}
          id="copy"
          label={t("overlay.copy")}
          tip={withKey(t("overlay.copy"), "copy")}
          primary={view.primary === "copy"}
          disabled={busy}
          onClick={() => void editor.finish("copy")}
        >
          {spinnerOr("copy", <CopyIcon />)}
        </ToolButton>
        {setOpen && (
          <ToolButton {...common}
            id="copyAll"
            label={t("overlay.copyAll", { count: view.setCount + 1 })}
            tip={withKey(t("overlay.copyAllHint"), "primary")}
            text={t("overlay.copyAll", { count: view.setCount + 1 })}
            primary
            disabled={busy}
            onClick={() => void editor.finish("copyAll")}
          >
            {spinnerOr("copyAll", <CopyIcon />)}
            {(vertical || compact) && <span className="count-badge">{view.setCount + 1}</span>}
          </ToolButton>
        )}
        <ToolButton {...common} id="close" label={t("overlay.close")} tip={`${t("overlay.close")} (Esc)`} disabled={busy} onClick={() => editor.cancel()}>
          <CloseIcon />
        </ToolButton>
      </div>

      {place && !vertical && place.side !== "inside" && view.markCount > 0 && hint && (
        <ToolbarHint place={place} screen={view.screen} toolbarHeight={sizes.current.horizontal?.h ?? 40} text={hint} />
      )}

      {tip && tipAnchor && !view.popover && (
        <Floating anchor={tipAnchor} order={side} screen={view.screen} className="tip glass" role="tooltip">
          <span>{tip.text}</span>
          {tip.sub && <span className="tip-sub">{tip.sub}</span>}
        </Floating>
      )}

      {view.popover === "color" && popAnchor && ctx && (
        <Floating anchor={popAnchor} order={side} screen={view.screen} className="popover glass" role="dialog">
          <div className="swatches" role="group" aria-label={t("overlay.color")}>
            {swatches.map((c) => (
              <button
                key={c}
                type="button"
                className="swatch"
                aria-label={t(`overlay.colors.${c}`)}
                aria-pressed={ctx.color.toUpperCase() === c}
                title={t(`overlay.colors.${c}`)}
                onMouseDown={keepFocus}
                onClick={() => editor.setColor(c)}
              >
                <span style={{ background: c }} />
              </button>
            ))}
          </div>
        </Floating>
      )}

      {view.popover === "width" && popAnchor && ctx && ctx.size !== null && (
        <Floating anchor={popAnchor} order={side} screen={view.screen} className="popover glass" role="dialog">
          <div className="widths" role="group" aria-label={t("overlay.width")}>
            {WIDTHS.map((w) => (
              <button
                key={w}
                type="button"
                className="width-opt"
                aria-label={t(`overlay.widths.${w}`)}
                aria-pressed={ctx.size === w}
                title={t(`overlay.widths.${w}`)}
                onMouseDown={keepFocus}
                onClick={() => editor.setSize(w)}
              >
                <WidthIcon weight={WIDTH_WEIGHT[w]} />
              </button>
            ))}
          </div>
          {ctx.plate !== null && (
            <button
              type="button"
              className="plate-toggle"
              role="switch"
              aria-checked={ctx.plate}
              onMouseDown={keepFocus}
              onClick={() => editor.setTextPlate(!ctx.plate)}
            >
              <PlateIcon />
              <span>{t("overlay.textPlate")}</span>
              <span className="switch" aria-hidden="true" />
            </button>
          )}
        </Floating>
      )}
    </>
  );
}

interface ButtonProps {
  id: string;
  label: string;
  tip?: string;
  sub?: string;
  pressed?: boolean;
  disabled?: boolean;
  primary?: boolean;
  text?: string;
  haspopup?: boolean;
  onClick: () => void;
  children: ReactNode;
  iconOnly: boolean;
  popover: string | null;
  register: (id: string, el: HTMLButtonElement | null) => void;
  onTip: (tip: Tip) => void;
  onUntip: () => void;
  onRetip: (tip: Tip) => void;
}

function ToolButton(p: ButtonProps) {
  const labeled = !!p.text && !p.iconOnly;
  const tip = { id: p.id, text: p.tip ?? p.label, sub: p.sub };
  useEffect(() => p.onRetip(tip), [tip.text, tip.sub]);
  return (
    <button
      ref={(el) => p.register(p.id, el)}
      type="button"
      className={`tb-btn${p.primary ? " primary" : ""}${labeled ? " labeled" : ""}`}
      aria-label={p.label}
      aria-pressed={p.pressed}
      aria-haspopup={p.haspopup ? "dialog" : undefined}
      aria-expanded={p.haspopup ? p.popover === p.id : undefined}
      disabled={p.disabled}
      onMouseDown={keepFocus}
      onMouseEnter={() => p.onTip(tip)}
      onMouseLeave={p.onUntip}
      onFocus={() => p.onTip(tip)}
      onBlur={p.onUntip}
      onClick={() => {
        p.onUntip();
        p.onClick();
      }}
    >
      {p.children}
      {labeled && <span>{p.text}</span>}
    </button>
  );
}

function ToolbarHint({ place, screen, toolbarHeight, text }: { place: ToolbarPlacement; screen: Size; toolbarHeight: number; text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    const y = place.side === "above" ? place.y - 6 - box.height : place.y + toolbarHeight + 6;
    const x = Math.max(8, Math.min(screen.w - 8 - box.width, place.x));
    const fits = y >= 8 && y + box.height <= screen.h - 8;
    const next = fits ? { x, y } : null;
    if (next?.x !== pos?.x || next?.y !== pos?.y) setPos(next);
  });
  return (
    <div ref={ref} className="set-hint" style={{ left: pos?.x ?? 0, top: pos?.y ?? 0, visibility: pos ? "visible" : "hidden" }}>
      {text}
    </div>
  );
}
