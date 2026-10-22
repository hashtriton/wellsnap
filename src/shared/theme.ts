import { useEffect } from "react";
import { api, events, type Theme } from "./ipc";

export function applyTheme(theme: Theme): void {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}

/** Keeps <html data-theme> in sync with the app setting for the lifetime of a window. */
export function useThemeSync(): void {
  useEffect(() => {
    api.getSettings().then((s) => applyTheme(s.theme));
    const unlisten = events.onSettingsChanged((s) => applyTheme(s.theme));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
}
