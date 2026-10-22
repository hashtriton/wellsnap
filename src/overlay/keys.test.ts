import { describe, expect, it } from "vitest";
import {
  escapeStep,
  formatBinding,
  HABITS,
  inputKeyAction,
  isRepeatable,
  KEYMAP,
  matchHabit,
  matchKey,
  shortcutOf,
  type EscContext,
  type InputKeyEvent,
  type KeyInput,
} from "./keys";

const key = (code: string, mods: Partial<KeyInput> = {}, keyName = ""): KeyInput => ({
  code,
  key: keyName,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

describe("matchKey", () => {
  it("maps tool letters by physical key, whatever the layout", () => {
    expect(matchKey(key("KeyA", {}, "ф"), false)).toBe("tool.arrow");
    expect(matchKey(key("KeyN", {}, "т"), false)).toBe("tool.mark");
    expect(matchKey(key("KeyH"), false)).toBe("tool.marker");
  });

  it("separates Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y and plain Z", () => {
    expect(matchKey(key("KeyZ", { ctrlKey: true }), false)).toBe("undo");
    expect(matchKey(key("KeyZ", { ctrlKey: true, shiftKey: true }), false)).toBe("redo");
    expect(matchKey(key("KeyY", { ctrlKey: true }), false)).toBe("redo");
    expect(matchKey(key("KeyZ"), false)).toBe("loupe");
  });

  it("uses Cmd on macOS and ignores Ctrl there", () => {
    expect(matchKey(key("KeyC", { metaKey: true }), true)).toBe("copy");
    expect(matchKey(key("KeyC", { ctrlKey: true }), true)).toBeNull();
    expect(matchKey(key("KeyC", { metaKey: true }), false)).toBeNull();
  });

  it("maps Enter, Shift+Enter and the copy and save chords", () => {
    expect(matchKey(key("Enter"), false)).toBe("primary");
    expect(matchKey(key("NumpadEnter", { shiftKey: true }), false)).toBe("addToSet");
    expect(matchKey(key("KeyC", { ctrlKey: true }), false)).toBe("copy");
    expect(matchKey(key("KeyS", { ctrlKey: true }), false)).toBe("save");
    expect(matchKey(key("KeyA", { ctrlKey: true }), false)).toBe("selectAll");
    expect(matchKey(key("KeyC"), false)).toBe("pickColor");
  });

  it("maps arrows with modifiers", () => {
    expect(matchKey(key("ArrowLeft"), false)).toBe("move");
    expect(matchKey(key("ArrowUp", { shiftKey: true }), false)).toBe("moveFast");
    expect(matchKey(key("ArrowRight", { altKey: true }), false)).toBe("resize");
    expect(matchKey(key("ArrowDown", { altKey: true, shiftKey: true }), false)).toBe("resizeFast");
  });

  it("toggles the callout text placement with L on any layout", () => {
    expect(matchKey(key("KeyL", {}, "д"), false)).toBe("commentPlacement");
    expect(matchKey(key("", {}, "l"), false)).toBe("commentPlacement");
    expect(matchKey(key("KeyL", { ctrlKey: true }), false)).toBeNull();
    expect(shortcutOf("commentPlacement", false)).toBe("L");
    expect(shortcutOf("commentPlacement", true)).toBe("L");
  });

  it("toggles captions with Shift+T, plain T stays the text tool", () => {
    expect(matchKey(key("KeyT", { shiftKey: true }, "Е"), false)).toBe("captions");
    expect(matchKey(key("KeyT", {}, "е"), false)).toBe("tool.text");
    expect(matchKey(key("KeyT", { shiftKey: true, ctrlKey: true }), false)).toBeNull();
    expect(isRepeatable("captions")).toBe(false);
    expect(shortcutOf("captions", false)).toBe("Shift+T");
    expect(shortcutOf("captions", true)).toBe("⇧T");
  });

  it("falls back to the key when the code is empty", () => {
    expect(matchKey(key("", {}, "r"), false)).toBe("tool.rect");
  });

  it("has no ambiguous chords", () => {
    const seen = new Set<string>();
    for (const b of KEYMAP) {
      for (const c of b.chords) {
        const id = `${c.code}|${!!c.mod}|${!!c.shift}|${!!c.alt}`;
        expect(seen.has(id), id).toBe(false);
        seen.add(id);
      }
    }
  });
});

describe("Lightshot habits", () => {
  it("maps Ctrl+D, Ctrl+E and Ctrl+X on any layout", () => {
    expect(matchHabit(key("KeyD", { ctrlKey: true }, "в"), false)).toBe("upload");
    expect(matchHabit(key("KeyE", { ctrlKey: true }, "у"), false)).toBe("editOnline");
    expect(matchHabit(key("KeyX", { ctrlKey: true }, "ч"), false)).toBe("close");
    expect(matchHabit(key("", { ctrlKey: true }, "d"), false)).toBe("upload");
  });

  it("uses Cmd on macOS and ignores Ctrl there", () => {
    expect(matchHabit(key("KeyD", { metaKey: true }), true)).toBe("upload");
    expect(matchHabit(key("KeyE", { metaKey: true }), true)).toBe("editOnline");
    expect(matchHabit(key("KeyX", { metaKey: true }), true)).toBe("close");
    expect(matchHabit(key("KeyD", { ctrlKey: true }), true)).toBeNull();
    expect(matchHabit(key("KeyD", { metaKey: true }), false)).toBeNull();
  });

  it("needs exactly the modifier: plain or shifted letters stay free", () => {
    expect(matchHabit(key("KeyD"), false)).toBeNull();
    expect(matchHabit(key("KeyE"), false)).toBeNull();
    expect(matchHabit(key("KeyX"), false)).toBeNull();
    expect(matchHabit(key("KeyD", { ctrlKey: true, shiftKey: true }), false)).toBeNull();
    expect(matchHabit(key("KeyX", { ctrlKey: true, altKey: true }), false)).toBeNull();
    expect(matchHabit(key("KeyC", { ctrlKey: true }), false)).toBeNull();
  });

  it("never shadows a KEYMAP chord, and matchKey leaves them alone", () => {
    const chords = new Set(KEYMAP.flatMap((b) => b.chords.map((c) => `${c.code}|${!!c.mod}|${!!c.shift}|${!!c.alt}`)));
    for (const h of HABITS) {
      const c = h.chord;
      expect(chords.has(`${c.code}|${!!c.mod}|${!!c.shift}|${!!c.alt}`), h.id).toBe(false);
      for (const mac of [false, true]) {
        const press = key(c.code, mac ? { metaKey: true } : { ctrlKey: true });
        expect(matchKey(press, mac), h.id).toBeNull();
        expect(matchHabit(press, mac)).toBe(h.id);
      }
    }
    expect(matchKey(key("KeyD"), false)).toBeNull();
    expect(matchKey(key("KeyE"), false)).toBeNull();
  });
});

describe("isRepeatable", () => {
  it("lets nudges, undo and width steps repeat", () => {
    for (const id of ["move", "moveFast", "resize", "resizeFast", "undo", "redo", "widthDown", "widthUp"] as const) {
      expect(isRepeatable(id), id).toBe(true);
    }
  });

  it("keeps finishing, discarding and toggles one-shot", () => {
    for (const id of ["escape", "primary", "addToSet", "copy", "save", "delete", "selectAll", "loupe", "help", "commentPlacement", "pickColor", "tool.arrow"] as const) {
      expect(isRepeatable(id), id).toBe(false);
    }
  });
});

describe("shortcut labels", () => {
  it("formats for Windows and macOS", () => {
    expect(shortcutOf("copy", false)).toBe("Ctrl+C");
    expect(shortcutOf("copy", true)).toBe("⌘C");
    expect(shortcutOf("redo", false)).toBe("Ctrl+Shift+Z");
    expect(shortcutOf("tool.arrow", false)).toBe("A");
    expect(shortcutOf("addToSet", false)).toBe("Shift+Enter");
  });

  it("groups arrows and hides numpad duplicates", () => {
    const move = KEYMAP.find((b) => b.id === "moveFast")!;
    expect(formatBinding(move, false)).toEqual(["Shift+←↑→↓"]);
    const primary = KEYMAP.find((b) => b.id === "primary")!;
    expect(formatBinding(primary, false)).toEqual(["Enter"]);
    const del = KEYMAP.find((b) => b.id === "delete")!;
    expect(formatBinding(del, true)).toEqual(["⌫"]);
  });
});

describe("inputKeyAction", () => {
  const input = (k: string, code: string, mods: Partial<InputKeyEvent> = {}): InputKeyEvent => ({
    ...key(code, {}, k),
    repeat: false,
    isComposing: false,
    keyCode: 0,
    ...mods,
  });

  it("submits on Enter and commits on Esc", () => {
    expect(inputKeyAction(input("Enter", "Enter"), false, false)).toBe("submit");
    expect(inputKeyAction(input("Enter", "NumpadEnter"), false, false)).toBe("submit");
    expect(inputKeyAction(input("Escape", "Escape"), false, false)).toBe("close");
    expect(inputKeyAction(input("Enter", "Enter", { shiftKey: true }), false, false)).toBe("block");
  });

  it("leaves Enter and Esc to the IME while it composes", () => {
    expect(inputKeyAction(input("Enter", "Enter", { isComposing: true }), false, false)).toBeNull();
    expect(inputKeyAction(input("Escape", "Escape", { isComposing: true }), false, false)).toBeNull();
    expect(inputKeyAction(input("Process", "Enter", { keyCode: 229 }), false, false)).toBeNull();
    expect(inputKeyAction(input("Enter", "Enter", { keyCode: 229 }), false, false)).toBeNull();
    expect(inputKeyAction(input("Escape", "Escape", { keyCode: 229 }), false, false)).toBeNull();
  });

  it("swallows a held Enter or Esc", () => {
    expect(inputKeyAction(input("Enter", "Enter", { repeat: true }), false, false)).toBe("block");
    expect(inputKeyAction(input("Escape", "Escape", { repeat: true }), false, false)).toBe("block");
  });

  it("copies the snapshot on Ctrl+C only when no text is selected in the field", () => {
    expect(inputKeyAction(input("c", "KeyC", { ctrlKey: true }), false, false)).toBe("copy");
    expect(inputKeyAction(input("c", "KeyC", { ctrlKey: true }), false, true)).toBeNull();
    expect(inputKeyAction(input("c", "KeyC", { metaKey: true }), true, false)).toBe("copy");
    expect(inputKeyAction(input("c", "KeyC", { ctrlKey: true }), true, false)).toBeNull();
  });

  it("saves on Ctrl+S, also with text selected", () => {
    expect(inputKeyAction(input("s", "KeyS", { ctrlKey: true }), false, true)).toBe("save");
    expect(inputKeyAction(input("s", "KeyS", { metaKey: true }), true, false)).toBe("save");
  });

  it("leaves typing and the other editing keys to the input", () => {
    expect(inputKeyAction(input("T", "KeyT", { shiftKey: true }), false, false)).toBeNull();
    expect(inputKeyAction(input("a", "KeyA"), false, false)).toBeNull();
    expect(inputKeyAction(input("z", "KeyZ", { ctrlKey: true }), false, false)).toBeNull();
    expect(inputKeyAction(input("v", "KeyV", { ctrlKey: true }), false, false)).toBeNull();
  });
});

describe("escapeStep", () => {
  const base: EscContext = {
    inputOpen: false,
    panelOpen: false,
    dragging: false,
    tool: "select",
    annotationSelected: false,
    hasAnnotations: false,
    armed: false,
  };

  it("peels layers in order", () => {
    let c: EscContext = { ...base, inputOpen: true, panelOpen: true, dragging: true, tool: "arrow", annotationSelected: true, hasAnnotations: true };
    const steps: string[] = [];
    for (let i = 0; i < 8; i++) {
      const step = escapeStep(c);
      steps.push(step);
      if (step === "cancel") break;
      if (step === "closeInput") c = { ...c, inputOpen: false };
      if (step === "closePanel") c = { ...c, panelOpen: false };
      if (step === "cancelDrag") c = { ...c, dragging: false };
      if (step === "leaveTool") c = { ...c, tool: "select" };
      if (step === "deselect") c = { ...c, annotationSelected: false };
      if (step === "arm") c = { ...c, armed: true };
    }
    expect(steps).toEqual(["closeInput", "closePanel", "cancelDrag", "leaveTool", "deselect", "arm", "cancel"]);
  });

  it("has no step for the placement switch: Esc still peels the same layers", () => {
    expect(KEYMAP.find((b) => b.id === "escape")!.chords).toEqual([{ code: "Escape" }]);
    expect(matchKey(key("Escape"), false)).toBe("escape");
    expect(escapeStep({ ...base, tool: "mark", hasAnnotations: true })).toBe("leaveTool");
    expect(escapeStep({ ...base, inputOpen: true, tool: "mark", hasAnnotations: true })).toBe("closeInput");
  });

  it("cancels at once without annotations", () => {
    expect(escapeStep(base)).toBe("cancel");
  });

  it("asks for a second Esc when there is work to lose", () => {
    expect(escapeStep({ ...base, hasAnnotations: true })).toBe("arm");
    expect(escapeStep({ ...base, hasAnnotations: true, armed: true })).toBe("cancel");
  });
});
