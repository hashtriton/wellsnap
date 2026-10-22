// Snapshot strip: a narrow floating panel docked at the right edge while a multi-snapshot set exists.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppMark } from "../assets/AppMark";
import { useI18n, type TFunction } from "../shared/i18n";
import { api, events, snapshotUrl, type SetInfo, type Snapshot } from "../shared/ipc";
import { useThemeSync } from "../shared/theme";
import { formatAccelerator, guessPlatform } from "../shared/ui/hotkey";
import { Icon } from "../shared/ui/icons";
import { Spinner } from "../shared/ui/controls";
import { commentCount, totalComments } from "./counts";

type Busy = "copy" | "save" | "new" | "clear" | null;

export function App() {
  useThemeSync();
  const { t } = useI18n();
  const platform = useMemo(() => guessPlatform(navigator.userAgent), []);
  const [set, setSet] = useState<SetInfo | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [failed, setFailed] = useState(false);
  const failTimer = useRef<number | undefined>(undefined);

  const refresh = useCallback(() => {
    api
      .getSet()
      .then(setSet)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refresh();
    const subs = [events.onSessionChanged(refresh), events.onHotkeysStatus(refresh), events.onSettingsChanged(refresh)];
    return () => {
      subs.forEach((p) => p.then((f) => f()));
      window.clearTimeout(failTimer.current);
    };
  }, [refresh]);

  const snapshots = set?.snapshots ?? [];
  useEffect(() => {
    if (snapshots.length === 0) setConfirmClear(false);
  }, [snapshots.length]);

  const run = async (kind: Exclude<Busy, null>, action: () => Promise<unknown>) => {
    setBusy(kind);
    setFailed(false);
    try {
      await action();
    } catch {
      setFailed(true);
      window.clearTimeout(failTimer.current);
      failTimer.current = window.setTimeout(() => setFailed(false), 4000);
    } finally {
      setBusy(null);
    }
  };

  const comments = totalComments(snapshots);
  const hotkey = set ? formatAccelerator(set.smartPasteHotkey, platform) : "";

  return (
    <main className="strip" aria-label={t("strip.region")}>
      <header className="strip-head" data-tauri-drag-region="deep" title={t("strip.moveHint")}>
        <AppMark size={20} />
        <div className="strip-counts">
          <strong>{t("common.snapshots", { count: snapshots.length })}</strong>
          <span>{comments > 0 ? t("common.comments", { count: comments }) : t("strip.noComments")}</span>
        </div>
      </header>

      <ul className="strip-list">
        {set?.sessionId &&
          snapshots.map((snap) => (
            <SnapshotItem key={snap.letter} snap={snap} sessionId={set.sessionId!} t={t} onRemove={() => api.setRemove(snap.letter)} />
          ))}
      </ul>

      <footer className="strip-foot">
        <button
          type="button"
          className="strip-primary"
          title={t("strip.copyAllHint")}
          disabled={busy !== null || snapshots.length === 0}
          aria-busy={busy === "copy" || undefined}
          onClick={() => run("copy", api.setCopyAll)}
        >
          {busy === "copy" ? <Spinner /> : <span>{t("strip.copyAll")}</span>}
        </button>

        {confirmClear ? (
          <div className="strip-confirm" role="group" aria-label={t("strip.clearConfirm")}>
            <span>{t("strip.clearConfirm")}</span>
            <div>
              <button
                type="button"
                className="strip-confirm-yes"
                autoFocus
                disabled={busy !== null}
                onClick={() => run("clear", api.setClear).then(() => setConfirmClear(false))}
              >
                {t("strip.clear")}
              </button>
              <button type="button" className="strip-confirm-no" onClick={() => setConfirmClear(false)}>
                {t("strip.clearCancel")}
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="strip-tools">
              <ToolButton
                icon="capture"
                label={t("strip.newSnapshot")}
                hint={t("strip.newSnapshotHint")}
                busy={busy === "new"}
                disabled={busy !== null || set?.nextLetter === null}
                onClick={() => run("new", api.startCapture)}
              />
              <ToolButton
                icon="save"
                label={t("strip.saveAll")}
                hint={t("strip.saveAllHint")}
                busy={busy === "save"}
                disabled={busy !== null || snapshots.length === 0}
                onClick={() => run("save", api.setSaveAll)}
              />
              <ToolButton
                icon="trash"
                label={t("strip.clear")}
                hint={t("strip.clearHint")}
                danger
                disabled={busy !== null || snapshots.length === 0}
                onClick={() => setConfirmClear(true)}
              />
            </div>
            {failed ? (
              <p className="strip-hint strip-hint-error" role="status">
                {t("strip.failed")}
              </p>
            ) : (
              set && (
                <button
                  type="button"
                  className={`strip-hint${set.smartPasteRegistered ? "" : " strip-hint-off"}`}
                  title={set.smartPasteRegistered ? t("strip.smartPasteHint") : t("strip.smartPasteOff", { hotkey })}
                  onClick={() => api.openSettings("hotkeys")}
                >
                  <kbd>{hotkey}</kbd>
                  <span>{t("strip.smartPaste")}</span>
                </button>
              )
            )}
          </>
        )}
      </footer>
    </main>
  );
}

function SnapshotItem({ snap, sessionId, t, onRemove }: { snap: Snapshot; sessionId: string; t: TFunction; onRemove: () => void }) {
  const count = commentCount(snap);
  const label = t("strip.snapshot", { letter: snap.letter });
  return (
    <li className="strip-item">
      <figure className="strip-thumb" title={count > 0 ? `${label}, ${t("common.comments", { count })}` : label}>
        <img src={snapshotUrl(sessionId, snap.letter, snap.revision)} alt={label} draggable={false} decoding="async" />
        <span className="strip-letter" aria-hidden="true">
          {snap.letter}
        </span>
        {count > 0 && (
          <span className="strip-comments" aria-hidden="true">
            <Icon name="message" size={11} />
            {count}
          </span>
        )}
      </figure>
      <button type="button" className="strip-remove" aria-label={t("strip.remove", { letter: snap.letter })} title={t("strip.remove", { letter: snap.letter })} onClick={onRemove}>
        <Icon name="x" size={13} />
      </button>
    </li>
  );
}

function ToolButton({
  icon,
  label,
  hint,
  busy,
  danger,
  disabled,
  onClick,
}: {
  icon: "capture" | "save" | "trash";
  label: string;
  hint: string;
  busy?: boolean;
  danger?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`strip-tool${danger ? " strip-tool-danger" : ""}`}
      aria-label={label}
      title={`${label}. ${hint}`}
      aria-busy={busy || undefined}
      disabled={disabled}
      onClick={onClick}
    >
      {busy ? <Spinner /> : <Icon name={icon} size={17} />}
    </button>
  );
}
