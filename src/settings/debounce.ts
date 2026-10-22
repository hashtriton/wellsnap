/**
 * Commits the last pushed value once it settles for `delayMs`, or right away on `flush()`. A slider uses it
 * so that a change made just before it unmounts is saved instead of dropped with the timer.
 */
export function debouncedCommit<T>(delayMs: number, commit: (value: T) => void): { push: (value: T) => void; flush: () => void } {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: { value: T } | null = null;
  const flush = () => {
    clearTimeout(timer);
    timer = undefined;
    if (!pending) return;
    const { value } = pending;
    pending = null;
    commit(value);
  };
  const push = (value: T) => {
    pending = { value };
    clearTimeout(timer);
    timer = setTimeout(flush, delayMs);
  };
  return { push, flush };
}
