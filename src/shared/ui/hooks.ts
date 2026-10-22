// Live settings and app info for the regular windows (settings, onboarding).
import { useCallback, useEffect, useRef, useState } from "react";
import { api, events, type AppInfo, type Settings } from "../ipc";

export type SettingsPatch = Parameters<typeof api.updateSettings>[0];

/** Settings mirrored from the core; `update` applies a patch right away and reports success. */
export function useSettings(): { settings: Settings | null; update: (patch: SettingsPatch) => Promise<boolean> } {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    let alive = true;
    api.getSettings().then((s) => alive && setSettings(s));
    const unlisten = events.onSettingsChanged((s) => setSettings(s));
    return () => {
      alive = false;
      unlisten.then((f) => f());
    };
  }, []);
  const update = useCallback(async (patch: SettingsPatch) => {
    try {
      setSettings(await api.updateSettings(patch));
      return true;
    } catch {
      setSettings(await api.getSettings().catch(() => null));
      return false;
    }
  }, []);
  return { settings, update };
}

/** App info, refreshed when the window gets focus again (the user may return from system settings). */
export function useAppInfo(): { info: AppInfo | null; refresh: () => void } {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const alive = useRef(true);
  const refresh = useCallback(() => {
    api
      .getAppInfo()
      .then((i) => alive.current && setInfo(i))
      .catch(() => undefined);
  }, []);
  useEffect(() => {
    alive.current = true;
    refresh();
    const unlisten = events.onHotkeysStatus((hotkeys) => setInfo((prev) => (prev ? { ...prev, hotkeys } : prev)));
    window.addEventListener("focus", refresh);
    return () => {
      alive.current = false;
      window.removeEventListener("focus", refresh);
      unlisten.then((f) => f());
    };
  }, [refresh]);
  return { info, refresh };
}
