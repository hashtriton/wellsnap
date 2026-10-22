import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { placeAround, type Side } from "../geometry";
import type { Rect, Size } from "../types";

interface Props {
  /** Anchor in CSS px, or a function that reads it from the DOM right before placement. */
  anchor: Rect | (() => Rect | null);
  order: readonly Side[];
  screen: Size;
  gap?: number;
  className?: string;
  role?: string;
  id?: string;
  children: ReactNode;
}

/** Absolutely positioned box placed next to an anchor; measured before paint, so it never jumps. */
export function Floating({ anchor, order, screen, gap = 8, className, role, id, children }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    const a = typeof anchor === "function" ? anchor() : anchor;
    if (!el || !a) return;
    const box = el.getBoundingClientRect();
    const p = placeAround(a, { w: box.width, h: box.height }, screen, order, gap);
    if (!pos || Math.abs(pos.x - p.x) > 0.5 || Math.abs(pos.y - p.y) > 0.5) setPos({ x: p.x, y: p.y });
  });

  return (
    <div
      ref={ref}
      id={id}
      role={role}
      className={className}
      // Opacity rather than visibility while unplaced: a hidden input could not take focus on mount.
      style={{ left: pos?.x ?? 0, top: pos?.y ?? 0, opacity: pos ? 1 : 0 }}
    >
      {children}
    </div>
  );
}

export function rectOf(el: Element | null | undefined): Rect | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
}
