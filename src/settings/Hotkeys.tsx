import { useId, useState, type ReactNode } from "react";
import type { Platform } from "../shared/ipc";
import { api } from "../shared/ipc";
import type { TFunction } from "../shared/i18n";
import { CaptureFix, ConflictCard, useConflicts } from "../shared/ui/ConflictCard";
import { alternativeKey, captureAdvice, fixSteps, isTaken, suspectsOf, takenText } from "../shared/ui/conflicts";
import { Button, Notice, Row } from "../shared/ui/controls";
import { defaultHotkeys, formatAccelerator, sameAccelerator } from "../shared/ui/hotkey";
import { Icon } from "../shared/ui/icons";
import { PrintScreenCard } from "../shared/ui/PrintScreenCard";
import { PageHead } from "./General";
import { HotkeyRecorder } from "./HotkeyRecorder";
import { captureStatus, smartPasteStatus, type RowStatus } from "./hotkeyStatus";
import { openLink, pasteTargetsUrl } from "./links";
import type { PageProps } from "./types";

interface Fix {
  steps: string[];
  alternative: string | null;
  onRecheck: () => Promise<void>;
}

export function Hotkeys({ t, lang, platform, settings, info, update, refreshInfo }: PageProps) {
  const defaults = defaultHotkeys(platform);
  const status = info?.hotkeys;
  const [openFailed, setOpenFailed] = useState(false);
  const conflicts = useConflicts(platform, settings.captureHotkey);
  const advice = captureAdvice({
    hotkey: settings.captureHotkey,
    status,
    printScreen: conflicts.printScreen,
    report: conflicts.report,
    platform,
  });

  const recheck = async () => {
    try {
      await api.hotkeysRecheck();
    } catch {
      // The status line is re-read below either way; the published status also rescans the conflicts.
    }
    refreshInfo();
  };
  // Windows: re-registering helps whenever another program may have held the key. "other" counts too, since
  // a core that does not classify errors yet reports a taken key that way. macOS keeps its usual texts.
  const fix: Fix | undefined =
    isTaken(advice) || (platform === "windows" && advice?.kind === "other")
      ? {
          steps: fixSteps(suspectsOf(advice), conflicts.printScreen, t),
          alternative: alternativeKey(advice, conflicts.report, [settings.captureHotkey, settings.smartPasteHotkey], platform),
          onRecheck: recheck,
        }
      : undefined;

  return (
    <>
      <PageHead title={t("settings.hotkeys.title")} lead={t("settings.hotkeys.lead")} />
      <section className="group">
        <HotkeyRow
          label={t("settings.hotkeys.capture")}
          description={t("settings.hotkeys.captureDesc")}
          value={settings.captureHotkey}
          fallback={defaults.capture}
          other={{ value: settings.smartPasteHotkey, label: t("settings.hotkeys.smartPaste") }}
          offerPrintScreen={platform === "windows"}
          status={captureStatus(status)}
          statusText={takenText(advice, formatAccelerator(settings.captureHotkey, platform), t)}
          fix={fix}
          platform={platform}
          t={t}
          save={(captureHotkey) => update({ captureHotkey })}
        />
        <HotkeyRow
          label={t("settings.hotkeys.smartPaste")}
          description={t("settings.hotkeys.smartPasteDesc")}
          link={
            <button type="button" className="link-btn" onClick={async () => setOpenFailed(!(await openLink(pasteTargetsUrl(lang))))}>
              {t("settings.hotkeys.smartPasteWhere")}
              <Icon name="external" size={13} />
            </button>
          }
          value={settings.smartPasteHotkey}
          fallback={defaults.smartPaste}
          other={{ value: settings.captureHotkey, label: t("settings.hotkeys.capture") }}
          status={smartPasteStatus(status)}
          platform={platform}
          t={t}
          save={(smartPasteHotkey) => update({ smartPasteHotkey })}
        />
      </section>
      {openFailed && <Notice tone="warning">{t("settings.support.openFailed")}</Notice>}

      {platform === "windows" && (
        <>
          <PrintScreenCard t={t} />
          <ConflictCard t={t} platform={platform} hotkey={settings.captureHotkey} advice={advice} printScreen={conflicts.printScreen} />
        </>
      )}
      {platform === "macos" && (
        <Notice tone="info">
          {t("settings.hotkeys.macSystem", {
            shot3: formatAccelerator("Cmd+Shift+3", platform),
            shot4: formatAccelerator("Cmd+Shift+4", platform),
            shot5: formatAccelerator("Cmd+Shift+5", platform),
            ours: formatAccelerator(defaults.capture, platform),
          })}
        </Notice>
      )}
    </>
  );
}

function HotkeyRow({
  label,
  description,
  link,
  value,
  fallback,
  other,
  offerPrintScreen,
  status,
  statusText: ownerText,
  fix,
  platform,
  t,
  save,
}: {
  label: string;
  description: string;
  /** Under the description, but not part of the recorder's accessible description. */
  link?: ReactNode;
  value: string;
  fallback: string;
  other: { value: string; label: string };
  offerPrintScreen?: boolean;
  status: RowStatus;
  /** Replaces the generic text, e.g. to name the program that holds the key. */
  statusText?: string | null;
  fix?: Fix;
  platform: Platform;
  t: TFunction;
  save: (accelerator: string) => Promise<boolean>;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const descId = useId();

  /** Checks for a clash with the other hotkey and the syntax, then saves. Returns true when saved. */
  const apply = async (accelerator: string): Promise<boolean> => {
    if (sameAccelerator(accelerator, value, platform)) {
      setError(null);
      return true;
    }
    if (sameAccelerator(accelerator, other.value, platform)) {
      setError(t("settings.hotkeys.conflict", { action: other.label }));
      return false;
    }
    setBusy(true);
    try {
      try {
        await api.validateHotkey(accelerator);
      } catch {
        setError(t("settings.hotkeys.invalid"));
        return false;
      }
      const ok = await save(accelerator);
      setError(ok ? null : t("settings.hotkeys.saveFailed"));
      return ok;
    } finally {
      setBusy(false);
    }
  };

  const statusText =
    ownerText ??
    (status === "ok"
      ? t("settings.hotkeys.statusOk")
      : status === "idle"
        ? t("settings.hotkeys.statusIdle")
        : status === "taken"
          ? t("settings.hotkeys.statusTaken")
          : status === "reserved"
            ? t("settings.hotkeys.statusReserved")
            : null);

  return (
    <Row
      label={label}
      description={
        <>
          <span id={descId}>{description}</span>
          {link && <div>{link}</div>}
        </>
      }
      stacked
    >
      {() => (
        <div className="hotkey-row">
          <div className="hotkey-controls">
            <HotkeyRecorder
              value={value}
              action={label}
              platform={platform}
              t={t}
              describedBy={descId}
              busy={busy}
              onRecord={apply}
              onInvalid={(reason) =>
                setError(
                  reason === "needsModifier"
                    ? t(platform === "macos" ? "settings.hotkeys.needsModifierMac" : "settings.hotkeys.needsModifierWin")
                    : reason === "reserved"
                      ? t("settings.hotkeys.reserved")
                      : t("settings.hotkeys.unsupportedKey"),
                )
              }
              onCancel={() => setError(null)}
            />
            {offerPrintScreen && !sameAccelerator(value, "PrintScreen", platform) && (
              <Button size="s" variant="ghost" icon="keyboard" disabled={busy} onClick={() => apply("PrintScreen")}>
                {t("settings.hotkeys.usePrintScreen")}
              </Button>
            )}
            {!sameAccelerator(value, fallback, platform) && (
              <Button
                size="s"
                variant="ghost"
                icon="reset"
                disabled={busy}
                title={t("settings.hotkeys.resetHint", { hotkey: formatAccelerator(fallback, platform) })}
                onClick={() => apply(fallback)}
              >
                {t("settings.hotkeys.reset")}
              </Button>
            )}
          </div>
          {error ? (
            <p className="hotkey-status hotkey-status-error" role="alert">
              {error}
            </p>
          ) : (
            statusText && (
              <p className={`hotkey-status hotkey-status-${status}`} aria-live="polite">
                <span className="status-dot" aria-hidden="true" />
                {statusText}
              </p>
            )
          )}
          {fix && (
            <CaptureFix
              t={t}
              platform={platform}
              steps={fix.steps}
              alternative={fix.alternative}
              busy={busy}
              collapsed
              onRecheck={fix.onRecheck}
              onUseAlternative={(accelerator) => void apply(accelerator)}
            />
          )}
        </div>
      )}
    </Row>
  );
}
