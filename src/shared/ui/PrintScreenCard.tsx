// Windows only: who gets the Print Screen key (Snipping Tool by default on Windows 11) and a one-click way
// to hand it to WellSnap and back. Used in Settings and in onboarding.
import { useCallback, useEffect, useState } from "react";
import { api, type PrintScreenStatus } from "../ipc";
import type { TFunction } from "../i18n";
import { Button, Notice } from "./controls";
import { Icon } from "./icons";

type Busy = "takeOver" | "restore" | null;

export function PrintScreenCard({ t }: { t: TFunction }) {
  const [status, setStatus] = useState<PrintScreenStatus | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [failed, setFailed] = useState(false);

  const refresh = useCallback(() => {
    api
      .printScreenStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  const run = async (action: Exclude<Busy, null>) => {
    setBusy(action);
    setFailed(false);
    try {
      setStatus(await (action === "takeOver" ? api.printScreenTakeOver() : api.printScreenRestore()));
    } catch {
      setFailed(true);
    } finally {
      setBusy(null);
    }
  };

  if (!status?.applicable) return null;

  return (
    <section className="card ps-card" aria-labelledby="ps-title">
      <div className="card-head">
        <span className="card-icon">
          <Icon name="keyboard" />
        </span>
        <h3 id="ps-title" className="card-title">
          {t("settings.printScreen.title")}
        </h3>
      </div>
      {status.snippingEnabled ? (
        <>
          <p className="card-text">{t("settings.printScreen.snipping")}</p>
          <div className="card-actions">
            <Button variant="primary" busy={busy === "takeOver"} disabled={busy !== null} onClick={() => run("takeOver")}>
              {t("settings.printScreen.takeOver")}
            </Button>
            <Button variant="ghost" icon="external" disabled={busy !== null} onClick={() => api.openSystemSettings("keyboard").catch(() => setFailed(true))}>
              {t("settings.printScreen.openWindows")}
            </Button>
          </div>
        </>
      ) : status.changedByUs ? (
        <>
          <p className="card-text">
            <Icon name="check" size={16} className="text-success inline-icon" />
            {t("settings.printScreen.takenOver")}
          </p>
          <div className="card-actions">
            <Button busy={busy === "restore"} disabled={busy !== null} icon="reset" onClick={() => run("restore")}>
              {t("settings.printScreen.restore")}
            </Button>
          </div>
        </>
      ) : (
        <p className="card-text">
          <Icon name="check" size={16} className="text-success inline-icon" />
          {t("settings.printScreen.free")}
        </p>
      )}
      {status.onedriveEnabled && <Notice tone="info">{t("settings.printScreen.onedrive")}</Notice>}
      {failed && <Notice tone="error">{t("settings.printScreen.failed")}</Notice>}
    </section>
  );
}
