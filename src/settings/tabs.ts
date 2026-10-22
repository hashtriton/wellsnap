export const TABS = ["general", "hotkeys", "screenshots", "support", "privacy", "about"] as const;
export type Tab = (typeof TABS)[number];

/** Tab from a URL hash ("#support") or a "settings:tab" payload ("support"); null when unknown. */
export function parseTab(value: string | null | undefined): Tab | null {
  const name = (value ?? "").replace(/^#/, "").trim().toLowerCase();
  return (TABS as readonly string[]).includes(name) ? (name as Tab) : null;
}
