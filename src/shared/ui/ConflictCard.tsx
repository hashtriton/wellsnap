// Windows only: other screenshot tools that hold or share the capture key, with their own steps to free it.
// WellSnap only re-registers or changes its own key; it never closes or reconfigures another program.
// Used in Settings > Shortcuts and in onboarding, next to PrintScreenCard.
import { useCallback, useEffect, useRef, useState } from "react";
import type { TFunction } from "../i18n";
import { api, type ConflictReport, type ContenderId, type Platform, type PrintScreenStatus } from "../ipc";
import { alternativeKey, doubleNotes, fixSteps, isTaken, suspectsOf, takenText, type CaptureAdvice } from "./conflicts";
import { Button, Notice } from "./controls";
import { formatAccelerator } from "./hotkey";
import { Icon } from "./icons";

/** The scan and the Print Screen setting, refreshed on mount, on focus and whenever `key` changes. */
export function useConflicts(
  platform: Platform,
  key: string,
): { report: ConflictReport | null; printScreen: PrintScreenStatus | null; refresh: () => void } {
  const [report, setReport] = useState<ConflictReport | null>(null);
  const [printScreen, setPrintScreen] = useState<PrintScreenStatus | null>(null);
  // Only the latest round may land: an older scan must not overwrite a newer one.
  const round = useRef(0);

  const refresh = useCallback(() => {
    if (platform !== "windows") return;
    const n = ++round.current;
    api
      .hotkeyConflicts()
      .then((r) => n === round.current && setReport(r))
      .catch(() => undefined);
    api
      .printScreenStatus()
      .then((s) => n === round.current && setPrintScreen(s))
      .catch(() => undefined);
  }, [platform]);

  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => {
      round.current++;
      window.removeEventListener("focus", refresh);
    };
  }, [refresh, key]);

  return { report, printScreen, refresh };
}

const HIDDEN_KEY = "wellsnap.hiddenConflicts";

function readHidden(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
  } catch {
    return [];
  }
}

function writeHidden(ids: string[]): void {
  try {
    localStorage.setItem(HIDDEN_KEY, JSON.stringify(ids));
  } catch {
    // Storage can be unavailable; the note then comes back next time, which is harmless.
  }
}

/**
 * "How to fix", "Check again" and "Use {key}" for a capture key that does not work. `collapsed` hides the
 * steps behind "How to fix" (the Settings row); otherwise they are shown at once (onboarding).
 */
export function CaptureFix({
  t,
  platform,
  steps,
  alternative,
  busy,
  collapsed,
  onRecheck,
  onUseAlternative,
}: {
  t: TFunction;
  platform: Platform;
  steps: string[];
  alternative: string | null;
  busy?: boolean;
  collapsed: boolean;
  onRecheck: () => Promise<void>;
  onUseAlternative: (accelerator: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [checking, setChecking] = useState(false);
  const recheck = async () => {
    setChecking(true);
    try {
      await onRecheck();
    } finally {
      setChecking(false);
    }
  };
  return (
    <>
      {!collapsed &&
        steps.map((text, i) => (
          <p key={i} className="card-text">
            {text}
          </p>
        ))}
      <div className="card-actions">
        {collapsed && steps.length > 0 && (
          <Button size="s" variant="ghost" icon="info" aria-expanded={open} onClick={() => setOpen(!open)}>
            {t("settings.hotkeys.howToFix")}
          </Button>
        )}
        <Button size="s" variant="ghost" icon="refresh" busy={checking} disabled={busy} onClick={recheck}>
          {t("settings.hotkeys.checkAgain")}
        </Button>
        {alternative && (
          <Button size="s" icon="keyboard" disabled={busy || checking} onClick={() => onUseAlternative(alternative)}>
            {t("settings.hotkeys.useAlternative", { hotkey: formatAccelerator(alternative, platform) })}
          </Button>
        )}
      </div>
      {collapsed && open && steps.length > 0 && (
        <Notice tone="info">
          {steps.map((text, i) => (
            <div key={i}>{text}</div>
          ))}
        </Notice>
      )}
    </>
  );
}

/**
 * Settings (`withFix` false): only the notes about tools that may react to the same key; the capture row names
 * the owner of a taken key. Onboarding (`withFix` true): also the taken key, its fix and the actions.
 */
export function ConflictCard({
  t,
  platform,
  hotkey,
  advice,
  report,
  printScreen,
  withFix,
  onRecheck,
  onUseAlternative,
}: {
  t: TFunction;
  platform: Platform;
  hotkey: string;
  advice: CaptureAdvice | null;
  report: ConflictReport | null;
  printScreen: PrintScreenStatus | null;
  withFix: boolean;
  onRecheck: () => Promise<void>;
  onUseAlternative: (accelerator: string) => void;
}) {
  const [hidden, setHidden] = useState<string[]>(readHidden);
  if (platform !== "windows") return null;

  const label = formatAccelerator(hotkey, platform);
  const fix = withFix && isTaken(advice);
  const notes = doubleNotes(advice, printScreen, hidden);
  if (!fix && notes.length === 0) return null;

  const hide = (id: ContenderId) => {
    const next = [...hidden.filter((h) => h !== id), id];
    setHidden(next);
    writeHidden(next);
  };

  return (
    <section className="card" aria-labelledby="conflicts-title">
      <div className="card-head">
        <span className="card-icon">
          <Icon name="alert" />
        </span>
        <h3 id="conflicts-title" className="card-title">
          {t("settings.conflicts.title")}
        </h3>
      </div>
      {fix && (
        <>
          <p className="card-text">{takenText(advice, label, t) ?? t("settings.hotkeys.statusTaken")}</p>
          <CaptureFix
            t={t}
            platform={platform}
            steps={fixSteps(suspectsOf(advice), printScreen, t)}
            alternative={alternativeKey(advice, report, hotkey, platform)}
            collapsed={false}
            onRecheck={onRecheck}
            onUseAlternative={onUseAlternative}
          />
        </>
      )}
      {notes.map((app) => (
        <Notice key={app.id} tone="info">
          {t("settings.conflicts.mayDouble", { app: app.name, hotkey: label })}{" "}
          <button type="button" className="link-btn" onClick={() => hide(app.id)}>
            {t("settings.conflicts.hide")}
          </button>
        </Notice>
      ))}
    </section>
  );
}
