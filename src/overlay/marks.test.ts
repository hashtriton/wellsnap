import { describe, expect, it } from "vitest";
import { calloutOf, calloutsOf, contrastText, markInCrop, markLabel, marksMeta, nextMarkNum, removeAnnotation, setCaption } from "./marks";
import type { Annotation, MarkAnn, RectAnn } from "./types";

const mark = (id: number, num: number, comment = "", p = { x: 0, y: 0 }): MarkAnn => ({
  kind: "mark",
  id,
  num,
  color: "#FF6A1A",
  p,
  tail: null,
  comment,
});
const rect = (id: number): Annotation => ({ kind: "rect", id, color: "#E53935", size: 4, r: { x: 0, y: 0, w: 10, h: 10 } });

describe("mark numbering", () => {
  it("starts at 1 and continues from the highest number", () => {
    expect(nextMarkNum([])).toBe(1);
    expect(nextMarkNum([rect(1)])).toBe(1);
    expect(nextMarkNum([mark(1, 1), mark(2, 4), rect(3)])).toBe(5);
  });

  it("renumbers higher marks down by one on delete", () => {
    const list = [mark(1, 1), rect(2), mark(3, 2), mark(4, 3)];
    const next = removeAnnotation(list, 3);
    expect(next.map((a) => (a.kind === "mark" ? a.num : "r"))).toEqual([1, "r", 2]);
    expect(nextMarkNum(next)).toBe(3);
  });

  it("does not renumber when a non-mark is deleted", () => {
    const list = [mark(1, 1), rect(2), mark(3, 2)];
    expect(removeAnnotation(list, 2).map((a) => (a.kind === "mark" ? a.num : 0))).toEqual([1, 2]);
  });

  it("does not mutate the input list", () => {
    const list = [mark(1, 1), mark(2, 2)];
    removeAnnotation(list, 1);
    expect((list[1] as MarkAnn).num).toBe(2);
  });

  it("returns marks ordered by number with trimmed comments", () => {
    expect(marksMeta([mark(1, 2, " b "), rect(2), mark(3, 1, "a")], { x: 0, y: 0, w: 100, h: 100 })).toEqual([
      { num: 1, comment: "a" },
      { num: 2, comment: "b" },
    ]);
  });

  it("leaves out marks whose badge center is outside the crop", () => {
    const crop = { x: 100, y: 100, w: 200, h: 100 };
    const list = [mark(1, 1, "in", { x: 150, y: 150 }), mark(2, 2, "out", { x: 50, y: 150 }), mark(3, 3, "edge", { x: 300, y: 200 })];
    expect(markInCrop(list[1] as MarkAnn, crop)).toBe(false);
    expect(marksMeta(list, crop)).toEqual([
      { num: 1, comment: "in" },
      { num: 3, comment: "edge" },
    ]);
  });
});

describe("captions", () => {
  const arrow = (id: number, num?: number, comment = "", a = { x: 10, y: 20 }): Annotation => ({
    kind: "arrow",
    id,
    color: "#E53935",
    size: 4,
    a,
    b: { x: 200, y: 20 },
    ...(num ? { caption: { num, comment } } : {}),
  });
  const frame = (id: number, num?: number, comment = ""): Annotation => ({
    kind: "rect",
    id,
    color: "#1E88E5",
    size: 4,
    r: { x: 30, y: 40, w: 100, h: 50 },
    ...(num ? { caption: { num, comment } } : {}),
  });
  const nums = (list: Annotation[]) => calloutsOf(list).map((c) => [c.id, c.num]);

  it("maps marks and captioned shapes to callouts, plain shapes to none", () => {
    const m = mark(1, 1, "m");
    expect(calloutOf(m)).toBe(m);
    expect(calloutOf(arrow(2))).toBeNull();
    expect(calloutOf(rect(3))).toBeNull();
    expect(calloutOf(arrow(2, 2, "a"))).toEqual({ id: 2, color: "#E53935", num: 2, comment: "a", p: { x: 10, y: 20 }, tail: { x: 200, y: 20 }, host: "arrow" });
    expect(calloutOf(frame(3, 3, "r"))).toEqual({ id: 3, color: "#1E88E5", num: 3, comment: "r", p: { x: 30, y: 40 }, tail: null, host: "rect" });
    expect(nums([m, arrow(2), frame(3, 2), rect(4)])).toEqual([
      [1, 1],
      [3, 2],
    ]);
  });

  it("continues the numbering over marks and captions", () => {
    expect(nextMarkNum([arrow(1, 3)])).toBe(4);
    expect(nextMarkNum([mark(1, 1), frame(2, 2), arrow(3)])).toBe(3);
  });

  it("renumbers marks and captions together on delete", () => {
    const list = [mark(1, 1), arrow(2, 2), frame(3, 3), mark(4, 4)];
    expect(nums(removeAnnotation(list, 1))).toEqual([
      [2, 1],
      [3, 2],
      [4, 3],
    ]);
    expect(nums(removeAnnotation(list, 2))).toEqual([
      [1, 1],
      [3, 2],
      [4, 3],
    ]);
  });

  it("adds, updates and removes a caption", () => {
    const list = [mark(1, 1), arrow(2), frame(3, 2, "frame")];
    const added = setCaption(list, 2, "");
    expect(nums(added)).toEqual([
      [1, 1],
      [2, 3],
      [3, 2],
    ]);
    const updated = setCaption(added, 2, "text");
    expect(calloutOf(updated[1])).toMatchObject({ num: 3, comment: "text" });
    const removed = setCaption(updated, 3, null);
    expect(removed[2]).toEqual(frame(3));
    expect("caption" in removed[2]).toBe(false);
    expect(nums(removed)).toEqual([
      [1, 1],
      [2, 2],
    ]);
    // A mark or an unknown id leaves the list as it is.
    expect(setCaption(list, 1, "x")).toEqual(list);
    expect(setCaption(list, 9, "x")).toEqual(list);
    expect(setCaption(list, 2, null)).toEqual(list);
    expect((list[2] as RectAnn).caption).toEqual({ num: 2, comment: "frame" });
  });

  it("puts captions into the text block in number order, without those outside the crop", () => {
    const crop = { x: 0, y: 0, w: 100, h: 100 };
    const list = [frame(1, 3, " frame "), arrow(2, 1, "arrow"), mark(3, 2, "mark", { x: 50, y: 50 }), arrow(4, 4, "out", { x: 150, y: 20 })];
    expect(marksMeta(list, crop)).toEqual([
      { num: 1, comment: "arrow" },
      { num: 2, comment: "mark" },
      { num: 3, comment: "frame" },
    ]);
  });
});

describe("labels", () => {
  it("prefixes the snapshot letter when there is one", () => {
    expect(markLabel("A", 1)).toBe("A1");
    expect(markLabel(null, 12)).toBe("12");
  });

  it("picks readable text colors for the palette", () => {
    expect(contrastText("#FF6A1A")).toBe("#FFFFFF");
    expect(contrastText("#E53935")).toBe("#FFFFFF");
    expect(contrastText("#212121")).toBe("#FFFFFF");
    expect(contrastText("#FDD835")).toBe("#000000");
    expect(contrastText("#FFFFFF")).toBe("#000000");
  });
});
