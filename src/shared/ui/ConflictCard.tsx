// Windows only: other screenshot tools that hold or share the capture key, with their own steps to free it.
// WellSnap only re-registers or changes its own key; it never closes or reconfigures another program.
// Used in Settings > Shortcuts and in onboarding, next to PrintScreenCard.
import { useEffect, useMemo, useState } from "react";
import type { TFunction } from "../i18n";
import { api, events, type ConflictReport, type ContenderId, type Platform, type PrintScreenStatus } from "../ipc";
import {
  conflictScanner,
  doubleNotes,
  fixSteps,
  isTaken,
  readHiddenNotes,
  suspectsOf,
  takenText,
  writeHiddenNotes,
  type CaptureAdvice,
  type ConflictScan,
} from "./conflicts";
import { Button, Notice } from "./controls";
import { formatAccelerator } from "./hotkey";
import { Icon } from "./icons";

/**
 * The scan and the Print Screen setting for the capture key `hotkey`, loaded on mount, on focus, when the key
 * changes and whenever the core publishes the hotkey status. The core publishes after every registration,
 * including "Check again" and PrintScreenCard's take-over and restore, so those need no refresh of their own.
 */
export function useConflicts(platform: Platform, hotkey: string): { report: ConflictReport | null; printScreen: PrintScreenStatus | null } {
  const [scan, setScan] = useState<ConflictScan | null>(null);
  const scanner = useMemo(
    () => (platform === "windows" ? conflictScanner({ report: api.hotkeyConflicts, printScreen: api.printScreenStatus }, setScan) : null),
    [platform],
  );

  useEffect(() => {
    if (!scanner) return;
    const refresh = () => scanner.refresh(hotkey);
    refresh();
    window.addEventListener("focus", refresh);
    const unlisten = events.onHotkeysStatus(refresh);
    return () => {
      scanner.stop();
      window.removeEventListener("focus", refresh);
      unlisten.then((f) => f());
    };
  }, [scanner, hotkey]);

  // Until the scanner publishes for a new key, the report of the old one must not show.
  return { report: scan?.hotkey === hotkey ? scan.report : null, printScreen: scan?.printScreen ?? null };
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

/** The taken key's fix in the card (onboarding); Settings shows it in the capture row instead. */
export interface CardFix {
  alternative: string | null;
  /** The alternative key is being saved. */
  busy: boolean;
  onRecheck: () => Promise<void>;
  onUseAlternative: (accelerator: string) => void;
}

/**
 * The notes about tools that may react to the same key and, with `fix` (onboarding), also the taken key, its
 * steps and the actions. In Settings the capture row names the owner of a taken key.
 */
export function ConflictCard({
  t,
  platform,
  hotkey,
  advice,
  printScreen,
  fix,
}: {
  t: TFunction;
  platform: Platform;
  hotkey: string;
  advice: CaptureAdvice | null;
  printScreen: PrintScreenStatus | null;
  fix?: CardFix;
}) {
  const [hidden, setHidden] = useState<string[]>(readHiddenNotes);
  if (platform !== "windows") return null;

  const label = formatAccelerator(hotkey, platform);
  const taken = fix && isTaken(advice) ? advice : null;
  const notes = doubleNotes(advice, printScreen, hidden);
  if (!taken && notes.length === 0) return null;

  const hide = (id: ContenderId) => {
    const next = [...hidden.filter((h) => h !== id), id];
    setHidden(next);
    writeHiddenNotes(next);
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
      {taken && fix && (
        <>
          <p className="card-text">{takenText(taken, label, t) ?? t("settings.hotkeys.statusTaken")}</p>
          <CaptureFix
            t={t}
            platform={platform}
            steps={fixSteps(suspectsOf(taken), printScreen, t)}
            alternative={fix.alternative}
            busy={fix.busy}
            collapsed={false}
            onRecheck={fix.onRecheck}
            onUseAlternative={fix.onUseAlternative}
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
