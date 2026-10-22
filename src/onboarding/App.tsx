// First-run onboarding: three short steps, Esc closes on any of them.
import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { AppMark } from "../assets/AppMark";
import { useI18n, type TFunction } from "../shared/i18n";
import { api, type AppInfo, type Platform, type PrintScreenStatus, type Settings } from "../shared/ipc";
import { useThemeSync } from "../shared/theme";
import { Button, Keys, Notice, Row, Switch } from "../shared/ui/controls";
import { useAppInfo, useSettings, type SettingsPatch } from "../shared/ui/hooks";
import { defaultHotkeys, formatAccelerator, guessPlatform } from "../shared/ui/hotkey";
import { Icon, type IconName } from "../shared/ui/icons";
import { PrintScreenCard } from "../shared/ui/PrintScreenCard";

const STEPS = 3;

export function App() {
  useThemeSync();
  const { t } = useI18n();
  const { settings, update } = useSettings();
  const { info, refresh } = useAppInfo();
  const platform = info?.platform ?? guessPlatform(navigator.userAgent);
  const [step, setStep] = useState(0);
  const finished = useRef(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const hotkey = settings?.captureHotkey ?? defaultHotkeys(platform).capture;

  const finish = useCallback(() => {
    if (finished.current) return;
    finished.current = true;
    api.onboardingComplete().catch(() => {
      finished.current = false;
    });
  }, []);

  // The core mirrors the document title into the native title bar, taskbar and Alt+Tab.
  const title = t("onboarding.windowTitle");
  useEffect(() => {
    document.title = title;
  }, [title]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        finish();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [finish]);

  const go = (next: number) => {
    setStep(next);
    requestAnimationFrame(() => headingRef.current?.focus());
  };

  return (
    <div className="onb">
      <header className="onb-top">
        <div className="onb-dots" aria-hidden="true">
          {Array.from({ length: STEPS }, (_, i) => (
            <span key={i} className={i === step ? "onb-dot onb-dot-on" : i < step ? "onb-dot onb-dot-done" : "onb-dot"} />
          ))}
        </div>
        <span className="onb-progress">{t("onboarding.progress", { step: step + 1, total: STEPS })}</span>
      </header>

      <main className="onb-body" key={step}>
        {step === 0 && <Welcome t={t} platform={platform} hotkey={hotkey} headingRef={headingRef} />}
        {step === 1 && (
          <Setup t={t} platform={platform} settings={settings} update={update} info={info} refresh={refresh} headingRef={headingRef} />
        )}
        {step === 2 && <TryIt t={t} platform={platform} hotkey={hotkey} headingRef={headingRef} />}
      </main>

      <footer className="onb-foot">
        <span className="onb-esc">{t("onboarding.escHint")}</span>
        <div className="onb-nav">
          {step > 0 && (
            <Button variant="ghost" icon="chevronLeft" onClick={() => go(step - 1)}>
              {t("onboarding.back")}
            </Button>
          )}
          {step < STEPS - 1 ? (
            <Button variant="primary" onClick={() => go(step + 1)}>
              {t("onboarding.next")}
            </Button>
          ) : (
            <Button variant="primary" icon="check" onClick={finish}>
              {t("onboarding.finish")}
            </Button>
          )}
        </div>
      </footer>
    </div>
  );
}

type HeadingRef = RefObject<HTMLHeadingElement | null>;

function Welcome({ t, platform, hotkey, headingRef }: { t: TFunction; platform: Platform; hotkey: string; headingRef: HeadingRef }) {
  const flow: { icon: IconName; title: string; desc: string }[] = [
    {
      icon: "capture",
      title: t("onboarding.welcome.step1", { hotkey: formatAccelerator(hotkey, platform) }),
      desc: t("onboarding.welcome.step1Desc"),
    },
    { icon: "arrow", title: t("onboarding.welcome.step2"), desc: t("onboarding.welcome.step2Desc") },
    {
      icon: "message",
      title: t("onboarding.welcome.step3"),
      desc: t("onboarding.welcome.step3Desc", { paste: formatAccelerator(platform === "macos" ? "Cmd+V" : "Ctrl+V", platform) }),
    },
  ];
  return (
    <section className="onb-welcome">
      <AppMark size={68} />
      <h1 ref={headingRef} tabIndex={-1}>
        {t("onboarding.welcome.title")}
      </h1>
      <p className="onb-lead">{t("onboarding.welcome.promise")}</p>
      <ol className="onb-flow">
        {flow.map((f, i) => (
          <li key={i}>
            <span className="onb-flow-num">{i + 1}</span>
            <Icon name={f.icon} size={22} className="onb-flow-icon" />
            <strong>{f.title}</strong>
            <span>{f.desc}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function Setup({
  t,
  platform,
  settings,
  update,
  info,
  refresh,
  headingRef,
}: {
  t: TFunction;
  platform: Platform;
  settings: Settings | null;
  update: (patch: SettingsPatch) => Promise<boolean>;
  info: AppInfo | null;
  refresh: () => void;
  headingRef: HeadingRef;
}) {
  const mac = platform === "macos";
  return (
    <section className="onb-setup">
      <h1 ref={headingRef} tabIndex={-1}>
        {t("onboarding.setup.title")}
      </h1>
      <p className="onb-lead">{t("onboarding.setup.lead")}</p>
      <div className="onb-stack">
        {mac ? <MacPermissions t={t} info={info} refresh={refresh} /> : <PrintScreenCard t={t} />}
        {settings && (
          <div className="card onb-row-card">
            <Row label={mac ? t("onboarding.setup.autostartMac") : t("onboarding.setup.autostart")} description={t("onboarding.setup.autostartDesc")}>
              {({ labelId, descId }) => (
                <Switch labelledBy={labelId} describedBy={descId} checked={settings.autostart} onChange={(autostart) => update({ autostart })} />
              )}
            </Row>
          </div>
        )}
        {settings && info?.telemetry && <TelemetryCard t={t} settings={settings} update={update} />}
        <div className="card">
          <div className="card-head">
            <span className="card-icon">
              <Icon name="tray" />
            </span>
            <h3 className="card-title">{t("onboarding.setup.trayTitle")}</h3>
          </div>
          <p className="card-text">{mac ? t("onboarding.setup.menuBar") : t("onboarding.setup.tray")}</p>
          {!mac && <p className="card-text">{t("onboarding.setup.trayWin11")}</p>}
        </div>
      </div>
    </section>
  );
}

/**
 * Crash reports and statistics are on by default; nothing is sent before onboarding closes, so this card is where
 * every user sees both switches first. The crash switch maps to "always" and "never"; "ask" is in Settings.
 */
function TelemetryCard({
  t,
  settings,
  update,
}: {
  t: TFunction;
  settings: Settings;
  update: (patch: SettingsPatch) => Promise<boolean>;
}) {
  return (
    <div className="card">
      <div className="card-head">
        <span className="card-icon">
          <Icon name="shield" />
        </span>
        <h3 className="card-title">{t("onboarding.setup.telemetryTitle")}</h3>
      </div>
      <p className="card-text">{t("onboarding.setup.telemetryText")}</p>
      <Row label={t("onboarding.setup.telemetryCrash")}>
        {({ labelId }) => (
          <Switch
            labelledBy={labelId}
            checked={settings.crashReports !== "never"}
            onChange={(on) => update({ crashReports: on ? "always" : "never" })}
          />
        )}
      </Row>
      <Row label={t("onboarding.setup.telemetryStats")}>
        {({ labelId }) => (
          <Switch labelledBy={labelId} checked={settings.usageStats} onChange={(usageStats) => update({ usageStats })} />
        )}
      </Row>
      <button type="button" className="link-btn" onClick={() => api.openSettings("privacy").catch(() => undefined)}>
        {t("onboarding.setup.telemetryDetails")}
      </button>
    </div>
  );
}

function MacPermissions({ t, info, refresh }: { t: TFunction; info: AppInfo | null; refresh: () => void }) {
  const [asking, setAsking] = useState(false);
  const granted = info?.screenPermission ?? false;
  const allow = async () => {
    setAsking(true);
    try {
      await api.requestScreenPermission();
    } catch {
      // The system prompt or System Settings is the source of truth; the status below is re-read.
    } finally {
      setAsking(false);
      refresh();
    }
  };
  const openPane = (kind: "screen" | "accessibility") => api.openSystemSettings(kind).catch(() => undefined);
  return (
    <>
      <div className="card">
        <div className="card-head">
          <span className="card-icon">
            <Icon name="capture" />
          </span>
          <h3 className="card-title">{t("onboarding.setup.screenTitle")}</h3>
          {info && <Status ok={granted} t={t} />}
        </div>
        <p className="card-text">{t("onboarding.setup.screenDesc")}</p>
        {!granted && (
          <>
            <div className="card-actions">
              <Button variant="primary" busy={asking} onClick={allow}>
                {t("onboarding.setup.allow")}
              </Button>
              <Button icon="external" onClick={() => openPane("screen")}>
                {t("onboarding.setup.openSettings")}
              </Button>
              <Button variant="ghost" icon="refresh" onClick={() => api.relaunch().catch(() => undefined)}>
                {t("onboarding.setup.relaunch")}
              </Button>
            </div>
            <p className="card-note">{t("onboarding.setup.relaunchHint")}</p>
          </>
        )}
        <p className="card-note">{t("onboarding.setup.monthly")}</p>
      </div>
      <div className="card">
        <div className="card-head">
          <span className="card-icon">
            <Icon name="keyboard" />
          </span>
          <h3 className="card-title">
            {t("onboarding.setup.accessTitle")} <span className="card-optional">{t("onboarding.setup.optional")}</span>
          </h3>
          {info && <Status ok={info.inputPermission} t={t} />}
        </div>
        <p className="card-text">{t("onboarding.setup.accessDesc", { paste: formatAccelerator("Cmd+V", "macos") })}</p>
        {info && !info.inputPermission && (
          <div className="card-actions">
            <Button icon="external" onClick={() => openPane("accessibility")}>
              {t("onboarding.setup.openSettings")}
            </Button>
          </div>
        )}
      </div>
    </>
  );
}

function Status({ ok, t }: { ok: boolean; t: TFunction }) {
  return (
    <span className={ok ? "pill pill-ok" : "pill pill-off"}>
      <Icon name={ok ? "check" : "alert"} size={13} />
      {ok ? t("onboarding.setup.granted") : t("onboarding.setup.notGranted")}
    </span>
  );
}

function TryIt({ t, platform, hotkey, headingRef }: { t: TFunction; platform: Platform; hotkey: string; headingRef: HeadingRef }) {
  const [busy, setBusy] = useState(false);
  const [printScreen, setPrintScreen] = useState<PrintScreenStatus | null>(null);
  useEffect(() => {
    if (platform !== "windows") return;
    const refresh = () => {
      api
        .printScreenStatus()
        .then(setPrintScreen)
        .catch(() => undefined);
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [platform]);
  // Windows 11 still gives Print Screen to Snipping Tool: "Press PrtSc" would open the wrong app.
  const snipping = hotkey.trim().toLowerCase() === "printscreen" && printScreen?.applicable && printScreen.snippingEnabled;
  const capture = async () => {
    setBusy(true);
    try {
      await api.startCapture();
    } catch {
      // A failed capture shows its own notice from the core.
    } finally {
      setBusy(false);
    }
  };
  const tips: { keys: string; text: string }[] = [
    { keys: "A", text: t("onboarding.tryIt.tipArrow") },
    { keys: "N", text: t("onboarding.tryIt.tipMark") },
    { keys: platform === "macos" ? "Cmd+C" : "Ctrl+C", text: t("onboarding.tryIt.tipCopy") },
  ];
  const [before, after] = t("onboarding.tryIt.press", { hotkey: "\u0000" }).split("\u0000");
  return (
    <section className="onb-try">
      <h1 ref={headingRef} tabIndex={-1}>
        {t("onboarding.tryIt.title")}
      </h1>
      {snipping ? (
        <div className="onb-snipping">
          <Notice tone="info">{t("onboarding.tryIt.snipping", { hotkey: formatAccelerator(hotkey, platform) })}</Notice>
        </div>
      ) : (
        <>
          <p className="onb-press">
            {before}
            <Keys accelerator={hotkey} platform={platform} />
            {after}
          </p>
          <span className="onb-or">{t("onboarding.tryIt.or")}</span>
        </>
      )}
      <Button variant="primary" icon="capture" busy={busy} onClick={capture}>
        {t("onboarding.tryIt.button")}
      </Button>
      <div className="onb-tips">
        <h2>{t("onboarding.tryIt.tipsTitle")}</h2>
        <ul>
          {tips.map((tip) => (
            <li key={tip.keys}>
              <Keys accelerator={tip.keys} platform={platform} size="s" />
              <span>{tip.text}</span>
            </li>
          ))}
        </ul>
      </div>
      <p className="onb-ready">{platform === "macos" ? t("onboarding.tryIt.readyMac") : t("onboarding.tryIt.readyWindows")}</p>
    </section>
  );
}
