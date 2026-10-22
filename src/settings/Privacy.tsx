// "Privacy" section: crash reports and anonymous statistics. A build without telemetry (forks, local builds) sends
// nothing, so it shows only that note and the local privacy text.
import { useCallback, useEffect, useState } from "react";
import type { TFunction } from "../shared/i18n";
import { api, type CrashMode, type Lang, type SentItem, type SentKind } from "../shared/ipc";
import { Button, Notice, Row, Segmented, Switch } from "../shared/ui/controls";
import { Icon } from "../shared/ui/icons";
import { PageHead } from "./General";
import { privacyKeys } from "./privacyTexts";
import type { PageProps } from "./types";

export const MODE_DESC: Record<CrashMode, string> = {
  always: "settings.privacy.crashAlwaysDesc",
  ask: "settings.privacy.crashAskDesc",
  never: "settings.privacy.crashNeverDesc",
};

export const SENT_KIND: Record<SentKind, string> = {
  crash: "settings.privacy.sentCrash",
  error: "settings.privacy.sentError",
  usage: "settings.privacy.sentUsage",
};

export function Privacy(props: PageProps) {
  const { t, info } = props;
  if (!info?.telemetry) {
    return (
      <>
        <PageHead title={t("settings.privacy.title")} />
        {info && (
          <>
            <Notice tone="info" icon="shield">
              {t("settings.privacy.unavailable")}
            </Notice>
            <p className="footnote">
              <Icon name="shield" size={14} />
              {t(privacyKeys(info).privacy)}
            </p>
          </>
        )}
      </>
    );
  }
  return <Telemetry {...props} />;
}

function Telemetry({ t, lang, settings, update }: PageProps) {
  const [pending, setPending] = useState(0);
  const [busy, setBusy] = useState<"send" | "delete" | null>(null);
  // null while the list is hidden.
  const [sent, setSent] = useState<SentItem[] | null>(null);
  const [resetDone, setResetDone] = useState(false);

  const refresh = useCallback(() => {
    api
      .telemetryStatus()
      .then((s) => setPending(s.pendingReports))
      .catch(() => undefined);
  }, []);

  // "Never" empties the outbox in the core, so the count is read again after a mode change.
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh, settings.crashReports]);

  const loadSent = () =>
    api
      .telemetryHistory()
      .then(setSent)
      .catch(() => setSent([]));

  const answer = async (choice: "send" | "delete") => {
    setBusy(choice);
    try {
      await api.crashPromptAnswer(choice);
    } catch {
      // The count below shows what is still waiting.
    } finally {
      setBusy(null);
      refresh();
      if (sent) loadSent();
    }
  };

  const resetIds = () => {
    api
      .telemetryResetIds()
      .then(() => setResetDone(true))
      .catch(() => undefined);
  };

  return (
    <>
      <PageHead title={t("settings.privacy.title")} lead={t("settings.privacy.lead")} />
      <section className="group">
        <Row label={t("settings.privacy.crash")} description={t(MODE_DESC[settings.crashReports])}>
          {({ labelId }) => (
            <div className="privacy-mode">
              <Segmented<CrashMode>
                labelledBy={labelId}
                value={settings.crashReports}
                onChange={(crashReports) => update({ crashReports })}
                options={[
                  { value: "always", label: t("settings.privacy.crashAlways") },
                  { value: "ask", label: t("settings.privacy.crashAsk") },
                  { value: "never", label: t("settings.privacy.crashNever") },
                ]}
              />
            </div>
          )}
        </Row>
        <p className="privacy-detail">{t("settings.privacy.crashDesc")}</p>
        {pending > 0 && (
          <Row label={t("settings.privacy.pending", { count: pending })}>
            {() => (
              <>
                <Button size="s" variant="primary" busy={busy === "send"} disabled={busy !== null} onClick={() => answer("send")}>
                  {t("settings.privacy.send")}
                </Button>
                <Button size="s" variant="ghost" icon="trash" busy={busy === "delete"} disabled={busy !== null} onClick={() => answer("delete")}>
                  {t("settings.privacy.delete")}
                </Button>
              </>
            )}
          </Row>
        )}
      </section>

      <section className="group">
        <Row label={t("settings.privacy.stats")} description={t("settings.privacy.statsDesc")}>
          {({ labelId, descId }) => (
            <Switch labelledBy={labelId} describedBy={descId} checked={settings.usageStats} onChange={(usageStats) => update({ usageStats })} />
          )}
        </Row>
        <Row
          label={t("settings.privacy.resetId")}
          description={resetDone ? t("settings.privacy.resetIdDone") : t("settings.privacy.resetIdDesc")}
        >
          {() => (
            <Button size="s" icon={resetDone ? "check" : "reset"} onClick={resetIds}>
              {t("settings.support.resetCounters")}
            </Button>
          )}
        </Row>
      </section>

      <section className="privacy-sent">
        <Button size="s" variant="ghost" icon={sent ? "x" : "info"} aria-expanded={sent !== null} onClick={() => (sent ? setSent(null) : loadSent())}>
          {t(sent ? "settings.privacy.hideSent" : "settings.privacy.showSent")}
        </Button>
        {sent && <SentList items={sent} t={t} lang={lang} />}
      </section>

      <section className="privacy-where">
        <h2 className="section-title">{t("settings.privacy.whereTitle")}</h2>
        <p className="footnote">
          <Icon name="shield" size={14} />
          {t("settings.privacy.where")}
        </p>
      </section>
    </>
  );
}

function SentList({ items, t, lang }: { items: SentItem[]; t: TFunction; lang: Lang }) {
  if (items.length === 0) return <p className="privacy-detail">{t("settings.privacy.nothingSent")}</p>;
  const time = new Intl.DateTimeFormat(lang, { dateStyle: "medium", timeStyle: "short" });
  return (
    <ul className="sent-list">
      {items.map((item, i) => {
        const line = (
          <>
            <span className="sent-time">{time.format(new Date(item.at * 1000))}</span>
            <strong>{t(SENT_KIND[item.kind])}</strong>
            <span className="sent-summary">{item.summary}</span>
          </>
        );
        return (
          <li key={`${item.at}-${i}`}>
            {item.detail ? (
              <details>
                <summary>{line}</summary>
                <pre>{item.detail}</pre>
              </details>
            ) : (
              <div className="sent-line">{line}</div>
            )}
          </li>
        );
      })}
    </ul>
  );
}
