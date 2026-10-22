import { afterEach, describe, expect, it, vi } from "vitest";
import { AutoHide, NUDGE_AUTO_HIDE_MS, outcomeOnHide, remainingAfterPause, UPDATED_AUTO_HIDE_MS } from "./nudge";

describe("nudge timing", () => {
  it("reports notSeen only for cards hidden within 2 s", () => {
    expect(outcomeOnHide(0)).toBe("notSeen");
    expect(outcomeOnHide(1999)).toBe("notSeen");
    expect(outcomeOnHide(2000)).toBe("later");
    expect(outcomeOnHide(NUDGE_AUTO_HIDE_MS)).toBe("later");
  });

  it("keeps a few seconds after a pause", () => {
    expect(remainingAfterPause(12_000, 3_000)).toBe(9_000);
    expect(remainingAfterPause(12_000, 11_000)).toBe(4_000);
    expect(remainingAfterPause(12_000, 30_000)).toBe(4_000);
  });
});

describe("updated pill auto-hide", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  const setup = () => {
    vi.useFakeTimers();
    const onExpire = vi.fn();
    const timer = new AutoHide(UPDATED_AUTO_HIDE_MS, onExpire, () => Date.now());
    timer.start();
    return { timer, onExpire };
  };

  it("hides after 8 s without a hover", () => {
    const { onExpire } = setup();
    vi.advanceTimersByTime(7_999);
    expect(onExpire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onExpire).toHaveBeenCalledTimes(1);
  });

  it("pauses on hover and resumes with the time left", () => {
    const { timer, onExpire } = setup();
    vi.advanceTimersByTime(2_000);
    timer.hold("hovered", true);
    vi.advanceTimersByTime(60_000);
    expect(onExpire).not.toHaveBeenCalled();
    timer.hold("hovered", false);
    vi.advanceTimersByTime(5_999);
    expect(onExpire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onExpire).toHaveBeenCalledTimes(1);
  });

  it("keeps a few seconds after a late hover and waits for every hold", () => {
    const { timer, onExpire } = setup();
    vi.advanceTimersByTime(7_500);
    timer.hold("hovered", true);
    timer.hold("focused", true);
    timer.hold("hovered", false);
    vi.advanceTimersByTime(30_000);
    expect(onExpire).not.toHaveBeenCalled();
    timer.hold("focused", false);
    vi.advanceTimersByTime(3_999);
    expect(onExpire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onExpire).toHaveBeenCalledTimes(1);
  });

  it("never fires after stop", () => {
    const { timer, onExpire } = setup();
    timer.stop();
    vi.advanceTimersByTime(60_000);
    expect(onExpire).not.toHaveBeenCalled();
  });
});
