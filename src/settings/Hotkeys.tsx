import { useState } from "react";
import type { Platform } from "../shared/ipc";
import { api } from "../shared/ipc";
import type { TFunction } from "../shared/i18n";
import { Button, Notice, Row } from "../shared/ui/controls";
import { defaultHotkeys, formatAccelerator, sameAccelerator } from "../shared/ui/hotkey";
import { PrintScreenCard } from "../shared/ui/PrintScreenCard";
import { PageHead } from "./General";
import { HotkeyRecorder } from "./HotkeyRecorder";
import { captureStatus, smartPasteStatus, type RowStatus } from "./hotkeyStatus";
import type { PageProps } from "./types";

export function Hotkeys({ t, platform, settings, info, update }: PageProps) {
  const defaults = defaultHotkeys(platform);
  const status = info?.hotkeys;
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
          platform={platform}
          t={t}
          save={(captureHotkey) => update({ captureHotkey })}
        />
        <HotkeyRow
          label={t("settings.hotkeys.smartPaste")}
          description={t("settings.hotkeys.smartPasteDesc")}
          value={settings.smartPasteHotkey}
          fallback={defaults.smartPaste}
          other={{ value: settings.captureHotkey, label: t("settings.hotkeys.capture") }}
          status={smartPasteStatus(status)}
          platform={platform}
          t={t}
          save={(smartPasteHotkey) => update({ smartPasteHotkey })}
        />
      </section>

      {platform === "windows" && <PrintScreenCard t={t} />}
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
  value,
  fallback,
  other,
  offerPrintScreen,
  status,
  platform,
  t,
  save,
}: {
  label: string;
  description: string;
  value: string;
  fallback: string;
  other: { value: string; label: string };
  offerPrintScreen?: boolean;
  status: RowStatus;
  platform: Platform;
  t: TFunction;
  save: (accelerator: string) => Promise<boolean>;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
    status === "ok"
      ? t("settings.hotkeys.statusOk")
      : status === "idle"
        ? t("settings.hotkeys.statusIdle")
        : status === "taken"
          ? t("settings.hotkeys.statusTaken")
          : status === "reserved"
            ? t("settings.hotkeys.statusReserved")
            : null;

  return (
    <Row label={label} description={description} stacked>
      {({ descId }) => (
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
        </div>
      )}
    </Row>
  );
}
