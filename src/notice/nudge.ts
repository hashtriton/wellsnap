import type { PromptOutcome } from "../shared/ipc";

/** The card hides itself after this long; that counts as "Later". */
export const NUDGE_AUTO_HIDE_MS = 12_000;
/** A card hidden sooner (e.g. by a new capture) was not really seen and is retried another day. */
export const NUDGE_MIN_VISIBLE_MS = 2_000;
/** Time left after the pointer or focus leaves a paused card, so it never vanishes right away. */
export const NUDGE_RESUME_MIN_MS = 4_000;

/** Outcome to report when the card disappears without a click. */
export function outcomeOnHide(visibleMs: number): Extract<PromptOutcome, "notSeen" | "later"> {
  return visibleMs < NUDGE_MIN_VISIBLE_MS ? "notSeen" : "later";
}

/** Remaining auto-hide time after a pause. */
export function remainingAfterPause(totalMs: number, elapsedMs: number): number {
  return Math.max(totalMs - elapsedMs, NUDGE_RESUME_MIN_MS);
}

/** The "updated" pill hides itself after this long unless the pointer or focus is on it. */
export const UPDATED_AUTO_HIDE_MS = 8_000;

/**
 * Auto-hide timer that pauses while any hold (hover, focus) is on and resumes with what was left, never less
 * than `NUDGE_RESUME_MIN_MS`.
 */
export class AutoHide {
  private id: ReturnType<typeof setTimeout> | undefined;
  private startedAt = 0;
  private holds = new Set<string>();

  constructor(
    private budget: number,
    private readonly onExpire: () => void,
    private readonly now: () => number = () => performance.now(),
  ) {}

  start() {
    clearTimeout(this.id);
    this.startedAt = this.now();
    this.id = setTimeout(() => {
      this.id = undefined;
      this.onExpire();
    }, this.budget);
  }

  hold(key: string, on: boolean) {
    if (on) this.holds.add(key);
    else this.holds.delete(key);
    if (this.holds.size > 0) this.pause();
    else if (this.id === undefined) this.start();
  }

  stop() {
    clearTimeout(this.id);
    this.id = undefined;
  }

  private pause() {
    if (this.id === undefined) return;
    this.stop();
    this.budget = remainingAfterPause(this.budget, this.now() - this.startedAt);
  }
}
