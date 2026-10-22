import { describe, expect, it } from "vitest";
import { History } from "./history";

describe("History", () => {
  it("undoes and redoes in order", () => {
    const h = new History(0);
    h.push(1);
    h.push(2);
    expect(h.undo()).toBe(true);
    expect(h.current).toBe(1);
    expect(h.redo()).toBe(true);
    expect(h.current).toBe(2);
    expect(h.redo()).toBe(false);
  });

  it("drops the redo branch on a new step", () => {
    const h = new History(0);
    h.push(1);
    h.undo();
    h.push(5);
    expect(h.canRedo).toBe(false);
    h.undo();
    expect(h.current).toBe(0);
    expect(h.canUndo).toBe(false);
  });

  it("keeps at most `limit` undo steps", () => {
    const h = new History(0, 100);
    for (let i = 1; i <= 150; i++) h.push(i);
    let steps = 0;
    while (h.undo()) steps++;
    expect(steps).toBe(100);
    expect(h.current).toBe(50);
  });

  it("supports at least 100 steps by default", () => {
    const h = new History(0);
    for (let i = 1; i <= 120; i++) h.push(i);
    let steps = 0;
    while (h.undo()) steps++;
    expect(steps).toBe(120);
  });

  it("replaces the current state without a new step", () => {
    const h = new History("a");
    h.push("b");
    h.replace("c");
    expect(h.current).toBe("c");
    h.undo();
    expect(h.current).toBe("a");
  });

  it("discards the last step without a redo entry", () => {
    const h = new History("a");
    h.push("b");
    h.push("c");
    expect(h.discard()).toBe(true);
    expect(h.current).toBe("b");
    expect(h.canRedo).toBe(false);
    h.undo();
    expect(h.current).toBe("a");
    expect(h.discard()).toBe(false);
    expect(h.current).toBe("a");
  });

  it("keeps the undo depth when it discards at the limit", () => {
    const h = new History(0, 3);
    for (let i = 1; i <= 5; i++) h.push(i);
    h.push(6);
    h.push(7);
    expect(h.discard()).toBe(true);
    expect(h.discard()).toBe(true);
    expect(h.current).toBe(5);
    const seen: number[] = [];
    while (h.undo()) seen.push(h.current);
    expect(seen).toEqual([4, 3, 2]);
  });

  it("does not bring back a dropped step after an undo", () => {
    const h = new History(0, 2);
    for (let i = 1; i <= 3; i++) h.push(i);
    h.undo();
    h.discard();
    expect(h.current).toBe(1);
    expect(h.undo()).toBe(false);
  });

  it("resets", () => {
    const h = new History(1);
    h.push(2);
    h.reset(9);
    expect(h.current).toBe(9);
    expect(h.canUndo).toBe(false);
  });
});
