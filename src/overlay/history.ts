/** Linear undo/redo over immutable states; keeps at most `limit` undo steps. */
export class History<T> {
  private past: T[] = [];
  private future: T[] = [];
  /** What the last two pushes dropped over the limit (undefined: nothing), for `discard` to put back. */
  private dropped: (T | undefined)[] = [];

  constructor(
    private present: T,
    private readonly limit = 200,
  ) {}

  get current(): T {
    return this.present;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  push(next: T): void {
    this.past.push(this.present);
    const dropped = this.past.length > this.limit ? this.past.shift() : undefined;
    this.dropped = [...this.dropped.slice(-1), dropped];
    this.present = next;
    this.future = [];
  }

  /** Replaces the current state without a new undo step (used to merge follow-up edits). */
  replace(next: T): void {
    this.present = next;
    this.future = [];
  }

  /**
   * Drops the current state for the previous one with no redo entry: the last step never happened. Right
   * after up to two pushes, the oldest steps they dropped over the limit come back too.
   */
  discard(): boolean {
    const prev = this.past.pop();
    if (prev === undefined) return false;
    const dropped = this.dropped.pop();
    if (dropped !== undefined) this.past.unshift(dropped);
    this.present = prev;
    this.future = [];
    return true;
  }

  undo(): boolean {
    const prev = this.past.pop();
    if (prev === undefined) return false;
    this.dropped = [];
    this.future.push(this.present);
    this.present = prev;
    return true;
  }

  redo(): boolean {
    const next = this.future.pop();
    if (next === undefined) return false;
    this.dropped = [];
    this.past.push(this.present);
    this.present = next;
    return true;
  }

  reset(initial: T): void {
    this.present = initial;
    this.past = [];
    this.future = [];
    this.dropped = [];
  }
}
