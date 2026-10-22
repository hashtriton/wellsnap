// Small controls shared by the regular windows (settings, onboarding, strip, notice). Styles: ui.css.
import { useId, useRef, type ButtonHTMLAttributes, type KeyboardEvent, type ReactNode } from "react";
import type { Platform } from "../ipc";
import { acceleratorParts } from "./hotkey";
import { Icon, type IconName } from "./icons";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "m" | "s";
  icon?: IconName;
  busy?: boolean;
};

export function Button({ variant = "secondary", size = "m", icon, busy, children, className, disabled, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      className={`btn btn-${variant} btn-${size}${className ? ` ${className}` : ""}`}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      {busy ? <Spinner /> : icon ? <Icon name={icon} size={size === "s" ? 15 : 17} /> : null}
      {children !== undefined && <span>{children}</span>}
    </button>
  );
}

export function Spinner() {
  return <span className="spinner" aria-hidden="true" />;
}

export function Switch({
  checked,
  onChange,
  disabled,
  labelledBy,
  describedBy,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  labelledBy?: string;
  describedBy?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={checked}
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-thumb" />
    </button>
  );
}

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
}

/** Radio group drawn as a segmented control; arrow keys move the selection, as native radios do. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  labelledBy,
  disabled,
}: {
  value: T;
  options: SegmentOption<T>[];
  onChange: (value: T) => void;
  labelledBy?: string;
  disabled?: boolean;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const current = Math.max(
    0,
    options.findIndex((o) => o.value === value),
  );
  const onKeyDown = (e: KeyboardEvent) => {
    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = (current + step + options.length) % options.length;
    onChange(options[next].value);
    refs.current[next]?.focus();
  };
  return (
    <div className="seg" role="radiogroup" aria-labelledby={labelledBy} onKeyDown={onKeyDown}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={i === current ? 0 : -1}
          className="seg-item"
          disabled={disabled}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Key caps of an accelerator: Ctrl + Alt + V, or the macOS symbols. */
export function Keys({ accelerator, platform, size = "m" }: { accelerator: string; platform: Platform; size?: "m" | "s" }) {
  return (
    <span className={`keys keys-${size}`}>
      {acceleratorParts(accelerator, platform).map((part, i) => (
        <kbd key={i} className="kbd">
          {part}
        </kbd>
      ))}
    </span>
  );
}

/** Label and description on the left, control on the right. */
export function Row({
  label,
  description,
  children,
  stacked,
}: {
  label: ReactNode;
  description?: ReactNode;
  children: (ids: { labelId: string; descId?: string }) => ReactNode;
  stacked?: boolean;
}) {
  const id = useId();
  const labelId = `${id}-label`;
  const descId = description ? `${id}-desc` : undefined;
  return (
    <div className={`row${stacked ? " row-stacked" : ""}`}>
      <div className="row-text">
        <div className="row-label" id={labelId}>
          {label}
        </div>
        {description && (
          <div className="row-desc" id={descId}>
            {description}
          </div>
        )}
      </div>
      <div className="row-control">{children({ labelId, descId })}</div>
    </div>
  );
}

export function Notice({ tone, icon, children }: { tone: "info" | "warning" | "error" | "success"; icon?: IconName; children: ReactNode }) {
  const fallback: Record<typeof tone, IconName> = { info: "info", warning: "alert", error: "alert", success: "check" };
  return (
    <div className={`inline-notice inline-notice-${tone}`} role={tone === "error" ? "alert" : undefined}>
      <Icon name={icon ?? fallback[tone]} size={16} />
      <div>{children}</div>
    </div>
  );
}
