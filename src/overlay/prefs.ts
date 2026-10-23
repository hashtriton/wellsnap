// Per-tool color and width, stored in settings.toolPrefs (the settings file is user-editable, so parse
// defensively).
import { DRAW_TOOLS, type BlurMode, type DrawTool } from "./types";

export const PALETTE = ["#E53935", "#FB8C00", "#FDD835", "#43A047", "#1E88E5", "#8E24AA", "#212121", "#FFFFFF"] as const;
/** Same as --mark in theme.css. */
export const MARK_COLOR = "#FF6A1A";
/** Width presets S/M/L in CSS px. */
export const WIDTHS = [2, 4, 8] as const;
export const SIZED_TOOLS: readonly DrawTool[] = ["arrow", "pencil", "rect", "marker", "text"];

export interface ToolPref {
  color: string;
  size: number;
}

/** Tools that can open a caption input right after a shape is drawn. */
export const CAPTION_TOOLS = ["arrow", "rect"] as const;
export type CaptionTool = (typeof CAPTION_TOOLS)[number];

export interface ToolPrefs {
  tools: Record<DrawTool, ToolPref>;
  blurMode: BlurMode;
  textPlate: boolean;
  /** Per tool: open a caption input after each drawn shape. */
  captions: Record<CaptionTool, boolean>;
}

const DEFAULT_COLORS: Record<DrawTool, string> = {
  arrow: "#E53935",
  pencil: "#E53935",
  rect: "#E53935",
  marker: "#FDD835",
  blur: "#212121",
  mark: MARK_COLOR,
  text: "#E53935",
};

export function defaultPrefs(): ToolPrefs {
  const tools = {} as Record<DrawTool, ToolPref>;
  for (const t of DRAW_TOOLS) tools[t] = { color: DEFAULT_COLORS[t], size: 4 };
  return { tools, blurMode: "mosaic", textPlate: false, captions: { arrow: true, rect: true } };
}

const isHex = (v: unknown): v is string => typeof v === "string" && /^#[0-9a-f]{6}$/i.test(v);
const isWidth = (v: unknown): v is number => (WIDTHS as readonly unknown[]).includes(v);

export function parsePrefs(raw: Record<string, unknown> | null | undefined): ToolPrefs {
  const prefs = defaultPrefs();
  if (!raw || typeof raw !== "object") return prefs;
  for (const t of DRAW_TOOLS) {
    const entry = raw[t];
    if (!entry || typeof entry !== "object") continue;
    const { color, size } = entry as Record<string, unknown>;
    if (isHex(color)) prefs.tools[t].color = color.toUpperCase();
    if (isWidth(size)) prefs.tools[t].size = size;
  }
  if (raw.blurMode === "mosaic" || raw.blurMode === "solid") prefs.blurMode = raw.blurMode;
  if (typeof raw.textPlate === "boolean") prefs.textPlate = raw.textPlate;
  const captions = raw.captions;
  if (captions && typeof captions === "object") {
    for (const t of CAPTION_TOOLS) {
      const on = (captions as Record<string, unknown>)[t];
      if (typeof on === "boolean") prefs.captions[t] = on;
    }
  }
  return prefs;
}

export function serializePrefs(p: ToolPrefs): Record<string, unknown> {
  return { ...p.tools, blurMode: p.blurMode, textPlate: p.textPlate, captions: { ...p.captions } };
}

/** Next width preset up (`dir` 1) or down (-1), clamped. */
export function stepWidth(size: number, dir: 1 | -1): number {
  const i = WIDTHS.indexOf(size as (typeof WIDTHS)[number]);
  const from = i < 0 ? 1 : i;
  return WIDTHS[Math.min(WIDTHS.length - 1, Math.max(0, from + dir))];
}
