import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { debouncedCommit } from "./debounce";

describe("debouncedCommit", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("commits only the last value once it settles", () => {
    const commit = vi.fn();
    const d = debouncedCommit(300, commit);
    d.push(50);
    vi.advanceTimersByTime(200);
    d.push(60);
    vi.advanceTimersByTime(299);
    expect(commit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(commit).toHaveBeenCalledExactlyOnceWith(60);
  });

  it("flushes a pending value at once and only once", () => {
    const commit = vi.fn();
    const d = debouncedCommit(300, commit);
    d.push(60);
    d.flush();
    expect(commit).toHaveBeenCalledExactlyOnceWith(60);
    vi.advanceTimersByTime(1000);
    d.flush();
    expect(commit).toHaveBeenCalledTimes(1);
  });

  it("does nothing on flush when nothing is pending", () => {
    const commit = vi.fn();
    debouncedCommit(300, commit).flush();
    expect(commit).not.toHaveBeenCalled();
  });
});
