// Key map of the editor. Matching uses KeyboardEvent.code, so shortcuts work on any keyboard layout
// (Russian layout gives key "ф" for KeyA). The F1 panel and the tooltips are generated from KEYMAP.
import type { Pt, Tool } from "./types";

export type CommandId =
  | "tool.select"
  | "tool.arrow"
  | "tool.pencil"
  | "tool.rect"
  | "tool.marker"
  | "tool.blur"
  | "tool.mark"
  | "tool.text"
  | "primary"
  | "addToSet"
  | "copy"
  | "save"
  | "pin"
  | "undo"
  | "redo"
  | "selectAll"
  | "delete"
  | "move"
  | "moveFast"
  | "resize"
  | "resizeFast"
  | "widthDown"
  | "widthUp"
  | "loupe"
  | "commentPlacement"
  | "captions"
  | "pickColor"
  | "help"
  | "escape";

export interface Chord {
  code: string;
  /** Ctrl on Windows, Cmd on macOS. */
  mod?: boolean;
  shift?: boolean;
  alt?: boolean;
}

export interface Binding {
  id: CommandId;
  chords: Chord[];
}

const ARROW_CODES = ["ArrowLeft", "ArrowUp", "ArrowRight", "ArrowDown"];
const arrows = (m: Omit<Chord, "code">): Chord[] => ARROW_CODES.map((code) => ({ code, ...m }));

export const KEYMAP: readonly Binding[] = [
  { id: "tool.select", chords: [{ code: "KeyV" }] },
  { id: "tool.arrow", chords: [{ code: "KeyA" }] },
  { id: "tool.pencil", chords: [{ code: "KeyP" }] },
  { id: "tool.rect", chords: [{ code: "KeyR" }] },
  { id: "tool.marker", chords: [{ code: "KeyM" }, { code: "KeyH" }] },
  { id: "tool.blur", chords: [{ code: "KeyB" }] },
  { id: "tool.mark", chords: [{ code: "KeyN" }] },
  { id: "tool.text", chords: [{ code: "KeyT" }] },
  { id: "primary", chords: [{ code: "Enter" }, { code: "NumpadEnter" }] },
  { id: "addToSet", chords: [{ code: "Enter", shift: true }, { code: "NumpadEnter", shift: true }] },
  { id: "copy", chords: [{ code: "KeyC", mod: true }] },
  { id: "save", chords: [{ code: "KeyS", mod: true }] },
  // F3 is Snipaste's pin key.
  { id: "pin", chords: [{ code: "KeyP", mod: true }, { code: "F3" }] },
  { id: "undo", chords: [{ code: "KeyZ", mod: true }] },
  { id: "redo", chords: [{ code: "KeyZ", mod: true, shift: true }, { code: "KeyY", mod: true }] },
  { id: "selectAll", chords: [{ code: "KeyA", mod: true }] },
  { id: "delete", chords: [{ code: "Delete" }, { code: "Backspace" }] },
  { id: "move", chords: arrows({}) },
  { id: "moveFast", chords: arrows({ shift: true }) },
  { id: "resize", chords: arrows({ alt: true }) },
  { id: "resizeFast", chords: arrows({ alt: true, shift: true }) },
  { id: "widthDown", chords: [{ code: "BracketLeft" }] },
  { id: "widthUp", chords: [{ code: "BracketRight" }] },
  { id: "loupe", chords: [{ code: "KeyZ" }] },
  { id: "commentPlacement", chords: [{ code: "KeyL" }] },
  { id: "captions", chords: [{ code: "KeyT", shift: true }] },
  { id: "pickColor", chords: [{ code: "KeyC" }] },
  { id: "help", chords: [{ code: "F1" }] },
  { id: "escape", chords: [{ code: "Escape" }] },
];

export interface KeyInput {
  code: string;
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/** Some input methods report an empty `code`; derive it from a Latin letter key then. */
function codeOf(e: KeyInput): string {
  if (e.code) return e.code;
  return /^[a-z]$/i.test(e.key) ? `Key${e.key.toUpperCase()}` : e.key;
}

/**
 * Commands that may run on key auto-repeat. Everything else is one-shot: a held Esc must not peel every
 * layer and then discard the snapshot, a held Enter must not finish it.
 */
const REPEATABLE: ReadonlySet<CommandId> = new Set<CommandId>(["move", "moveFast", "resize", "resizeFast", "undo", "redo", "widthDown", "widthUp"]);

export function isRepeatable(cmd: CommandId): boolean {
  return REPEATABLE.has(cmd);
}

function chordMatches(c: Chord, e: KeyInput, mac: boolean): boolean {
  if (mac ? e.ctrlKey : e.metaKey) return false;
  const mod = mac ? e.metaKey : e.ctrlKey;
  return c.code === codeOf(e) && !!c.mod === mod && !!c.shift === e.shiftKey && !!c.alt === e.altKey;
}

export function matchKey(e: KeyInput, mac: boolean): CommandId | null {
  for (const binding of KEYMAP) {
    for (const c of binding.chords) {
      if (chordMatches(c, e, mac)) return binding.id;
    }
  }
  return null;
}

/** Lightshot's upload (Ctrl+D), online editor (Ctrl+E) and close (Ctrl+X) keys, for users who come from it. */
export type HabitId = "upload" | "editOnline" | "close";

/**
 * Kept out of KEYMAP so the F1 panel and the tooltips do not list them; checked only when `matchKey` finds
 * nothing. WellSnap uploads nothing, so "upload" copies and explains, "editOnline" points to the tools here.
 */
export const HABITS: readonly { id: HabitId; chord: Chord }[] = [
  { id: "upload", chord: { code: "KeyD", mod: true } },
  { id: "editOnline", chord: { code: "KeyE", mod: true } },
  { id: "close", chord: { code: "KeyX", mod: true } },
];

export function matchHabit(e: KeyInput, mac: boolean): HabitId | null {
  return HABITS.find((h) => chordMatches(h.chord, e, mac))?.id ?? null;
}

export interface InputKeyEvent extends KeyInput {
  repeat: boolean;
  isComposing: boolean;
  keyCode: number;
}

export type InputKeyAction = "submit" | "close" | "copy" | "save" | "pin" | "block";

/**
 * What a key does in a comment or text input; null leaves it to the input. Enter and Esc belong to the IME
 * while it composes (some engines report only keyCode 229). The window shortcuts do not reach an input, so
 * copy (with no text selected in the field), save and pin are taken here: they commit the text, then finish.
 */
export function inputKeyAction(e: InputKeyEvent, mac: boolean, textSelected: boolean): InputKeyAction | null {
  if (e.key === "Enter" || e.key === "Escape") {
    if (e.isComposing || e.keyCode === 229) return null;
    // A key held from before the input opened (Enter on a selected callout) must not close it at once.
    if (e.repeat) return "block";
    if (e.key === "Escape") return "close";
    return e.shiftKey ? "block" : "submit";
  }
  const cmd = matchKey(e, mac);
  if (cmd === "copy" && !textSelected && !e.repeat) return "copy";
  if (cmd === "save" || cmd === "pin") return e.repeat ? "block" : cmd;
  return null;
}

export function arrowDelta(code: string): Pt {
  switch (code) {
    case "ArrowLeft":
      return { x: -1, y: 0 };
    case "ArrowRight":
      return { x: 1, y: 0 };
    case "ArrowUp":
      return { x: 0, y: -1 };
    case "ArrowDown":
      return { x: 0, y: 1 };
    default:
      return { x: 0, y: 0 };
  }
}

export const TOOL_COMMANDS: Record<Tool, CommandId> = {
  select: "tool.select",
  arrow: "tool.arrow",
  pencil: "tool.pencil",
  rect: "tool.rect",
  marker: "tool.marker",
  blur: "tool.blur",
  mark: "tool.mark",
  text: "tool.text",
};

const KEY_NAMES: Record<string, [win: string, mac: string]> = {
  Enter: ["Enter", "Return"],
  Escape: ["Esc", "Esc"],
  Delete: ["Delete", "Delete"],
  Backspace: ["Backspace", "⌫"],
  ArrowLeft: ["←", "←"],
  ArrowUp: ["↑", "↑"],
  ArrowRight: ["→", "→"],
  ArrowDown: ["↓", "↓"],
  BracketLeft: ["[", "["],
  BracketRight: ["]", "]"],
};

function keyName(code: string, mac: boolean): string {
  const named = KEY_NAMES[code];
  if (named) return named[mac ? 1 : 0];
  if (code.startsWith("Key")) return code.slice(3);
  return code;
}

function modifiers(c: Omit<Chord, "code">, mac: boolean): string[] {
  const parts: string[] = [];
  if (mac) {
    if (c.alt) parts.push("⌥");
    if (c.shift) parts.push("⇧");
    if (c.mod) parts.push("⌘");
  } else {
    if (c.mod) parts.push("Ctrl");
    if (c.shift) parts.push("Shift");
    if (c.alt) parts.push("Alt");
  }
  return parts;
}

function join(parts: string[], mac: boolean): string {
  return parts.join(mac ? "" : "+");
}

export function formatChord(c: Chord, mac: boolean): string {
  return join([...modifiers(c, mac), keyName(c.code, mac)], mac);
}

/** Display form of a binding: arrow groups collapse into "←↑→↓", numpad duplicates are dropped. */
export function formatBinding(b: Binding, mac: boolean): string[] {
  const chords = b.chords.filter((c) => !c.code.startsWith("Numpad"));
  if (chords.length === 4 && chords.every((c) => ARROW_CODES.includes(c.code))) {
    return [join([...modifiers(chords[0], mac), "←↑→↓"], mac)];
  }
  // Delete and Backspace do the same; show the key the platform calls "delete".
  if (b.id === "delete") return [keyName(mac ? "Backspace" : "Delete", mac)];
  return chords.map((c) => formatChord(c, mac));
}

export function shortcutOf(id: CommandId, mac: boolean): string {
  const b = KEYMAP.find((x) => x.id === id);
  return b ? formatBinding(b, mac)[0] : "";
}

export interface EscContext {
  inputOpen: boolean;
  panelOpen: boolean;
  dragging: boolean;
  tool: Tool;
  annotationSelected: boolean;
  hasAnnotations: boolean;
  /** The "Esc again" plate is showing. */
  armed: boolean;
}

export type EscStep = "closeInput" | "closePanel" | "cancelDrag" | "leaveTool" | "deselect" | "arm" | "cancel";

/** Esc peels one layer at a time; a snapshot with annotations needs a second Esc to be dropped. */
export function escapeStep(c: EscContext): EscStep {
  if (c.inputOpen) return "closeInput";
  if (c.panelOpen) return "closePanel";
  if (c.dragging) return "cancelDrag";
  if (c.tool !== "select") return "leaveTool";
  if (c.annotationSelected) return "deselect";
  if (c.hasAnnotations && !c.armed) return "arm";
  return "cancel";
}
