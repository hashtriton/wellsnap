// Settings window: sidebar with sections, every control applies immediately (no OK button).
import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { AppMark } from "../assets/AppMark";
import { useI18n } from "../shared/i18n";
import { events } from "../shared/ipc";
import { useThemeSync } from "../shared/theme";
import { useAppInfo, useSettings, type SettingsPatch } from "../shared/ui/hooks";
import { guessPlatform } from "../shared/ui/hotkey";
import { Icon, type IconName } from "../shared/ui/icons";
import { Notice } from "../shared/ui/controls";
import { About } from "./About";
import { General } from "./General";
import { Hotkeys } from "./Hotkeys";
import { Screenshots } from "./Screenshots";
import { Support } from "./Support";
import { parseTab, TABS, type Tab } from "./tabs";
import type { PageProps } from "./types";

const ICONS: Record<Tab, IconName> = {
  general: "sliders",
  hotkeys: "keyboard",
  screenshots: "image",
  support: "heart",
  about: "info",
};

const PAGES: Record<Tab, (p: PageProps) => ReactNode> = {
  general: General,
  hotkeys: Hotkeys,
  screenshots: Screenshots,
  support: Support,
  about: About,
};

export function App() {
  useThemeSync();
  const { lang, t } = useI18n();
  const { settings, update: rawUpdate } = useSettings();
  const { info, refresh: refreshInfo } = useAppInfo();
  const platform = info?.platform ?? guessPlatform(navigator.userAgent);
  const [tab, setTab] = useState<Tab>(() => parseTab(window.location.hash) ?? "general");
  const [failed, setFailed] = useState(false);
  const failTimer = useRef<number | undefined>(undefined);
  const mainRef = useRef<HTMLElement>(null);

  const goTo = useCallback((next: Tab) => {
    setTab(next);
    history.replaceState(null, "", `#${next}`);
  }, []);

  useEffect(() => {
    const onHash = () => {
      const next = parseTab(window.location.hash);
      if (next) setTab(next);
    };
    window.addEventListener("hashchange", onHash);
    const unlisten = events.onSettingsTab((name) => {
      const next = parseTab(name);
      if (next) goTo(next);
    });
    return () => {
      window.removeEventListener("hashchange", onHash);
      unlisten.then((f) => f());
      window.clearTimeout(failTimer.current);
    };
  }, [goTo]);

  useEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [tab]);

  // The core mirrors the document title into the native title bar, taskbar and Alt+Tab.
  const title = t("settings.windowTitle");
  useEffect(() => {
    document.title = title;
  }, [title]);

  const update = useCallback(
    async (patch: SettingsPatch) => {
      const ok = await rawUpdate(patch);
      setFailed(!ok);
      window.clearTimeout(failTimer.current);
      if (!ok) failTimer.current = window.setTimeout(() => setFailed(false), 5000);
      return ok;
    },
    [rawUpdate],
  );

  const onNavKey = (e: KeyboardEvent) => {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = TABS[(TABS.indexOf(tab) + step + TABS.length) % TABS.length];
    goTo(next);
    document.getElementById(`tab-${next}`)?.focus();
  };

  const Page = PAGES[tab];
  const props: PageProps | null = settings ? { t, lang, platform, settings, info, update, refreshInfo, goTo } : null;

  return (
    <div className="settings">
      <nav className="side" aria-label={t("settings.nav.label")}>
        <div className="side-brand">
          <AppMark size={28} />
          <span>{t("common.appName")}</span>
        </div>
        <div role="tablist" aria-orientation="vertical" onKeyDown={onNavKey}>
          {TABS.map((name) => (
            <button
              key={name}
              id={`tab-${name}`}
              type="button"
              role="tab"
              aria-selected={tab === name}
              aria-controls="page"
              tabIndex={tab === name ? 0 : -1}
              className="side-item"
              onClick={() => goTo(name)}
            >
              <Icon name={ICONS[name]} size={18} />
              <span>{t(`settings.nav.${name}`)}</span>
              {name === "support" && settings?.donations.status === "supporter" && (
                <span className="side-badge" aria-hidden="true">
                  <Icon name="check" size={11} />
                </span>
              )}
            </button>
          ))}
        </div>
      </nav>
      <main id="page" className="page" role="tabpanel" aria-labelledby={`tab-${tab}`} ref={mainRef}>
        <div className="page-inner">{props && <Page {...props} />}</div>
        {failed && (
          <div className="page-toast">
            <Notice tone="error">{t("settings.applyFailed")}</Notice>
          </div>
        )}
      </main>
    </div>
  );
}
