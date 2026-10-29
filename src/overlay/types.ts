// Editor model. Every coordinate is in physical pixels of the monitor; `size` is a width preset in CSS px
// (2/4/8) that the renderer multiplies by the monitor scale.

export interface Pt {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Size {
  w: number;
  h: number;
}

export const DRAW_TOOLS = ["arrow", "pencil", "rect", "marker", "blur", "mark", "text"] as const;
export type DrawTool = (typeof DRAW_TOOLS)[number];
export type Tool = "select" | DrawTool;
export type BlurMode = "mosaic" | "solid";

interface Common {
  id: number;
  color: string;
}

/**
 * Numbered comment of an arrow or a rectangle: a callout like a mark (same numbering, bubble and text
 * block), with its badge at the start of the arrow or the top-left corner of the rectangle.
 */
export interface Caption {
  num: number;
  comment: string;
  /** Bubble moved by hand: its top-left corner relative to the badge center. Absent: placed automatically. */
  bubble?: Pt;
}

export interface ArrowAnn extends Common {
  kind: "arrow";
  a: Pt;
  b: Pt;
  size: number;
  caption?: Caption;
}

export interface MarkerAnn extends Common {
  kind: "marker";
  a: Pt;
  b: Pt;
  size: number;
}

export interface PencilAnn extends Common {
  kind: "pencil";
  points: Pt[];
  size: number;
}

export interface RectAnn extends Common {
  kind: "rect";
  r: Rect;
  size: number;
  caption?: Caption;
}

export interface BlurAnn extends Common {
  kind: "blur";
  r: Rect;
  mode: BlurMode;
}

export interface MarkAnn extends Common {
  kind: "mark";
  num: number;
  /** Badge center. */
  p: Pt;
  /** End of the optional pointer line. */
  tail: Pt | null;
  comment: string;
  /** As in Caption. */
  bubble?: Pt;
}

export interface TextAnn extends Common {
  kind: "text";
  /** Top-left corner of the text box. */
  p: Pt;
  text: string;
  size: number;
  plate: boolean;
}

export type Annotation = ArrowAnn | MarkerAnn | PencilAnn | RectAnn | BlurAnn | MarkAnn | TextAnn;
export type AnnotationKind = Annotation["kind"];
