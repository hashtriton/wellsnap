import { useCallback, useEffect, useState } from "react";
import { AppMark } from "../assets/AppMark";
import type { TFunction } from "../shared/i18n";
import { api, events, type Lang, type Settings, type UpdateStatus } from "../shared/ipc";
import { RELEASES_URL, REPO_URL } from "../shared/donate";
import { Button, Notice, Row, Switch } from "../shared/ui/controls";
import { Icon, type IconName } from "../shared/ui/icons";
import { openLink } from "./links";
import { formatDate, pickNotes, stateAfterInstallError, updateView, type LocalUpdateState } from "./notes";
import { privacyKeys } from "./privacyTexts";
import type { PageProps } from "./types";

export function About({ t, lang, info, goTo, settings, update }: PageProps) {
  const [openFailed, setOpenFailed] = useState(false);
  const open = async (url: string) => setOpenFailed(!(await openLink(url)));
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const refreshStatus = useCallback(() => {
    api
      .getUpdate()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    refreshStatus();
    const unlisten = events.onUpdateStatus(setStatus);
    return () => {
      unlisten.then((f) => f());
    };
  }, [refreshStatus]);

  const links: { icon: IconName; label: string; onClick: () => void }[] = [
    { icon: "code", label: t("settings.about.repo"), onClick: () => open(REPO_URL) },
    { icon: "refresh", label: t("settings.about.releases"), onClick: () => open(RELEASES_URL) },
    { icon: "message", label: t("settings.about.feedback"), onClick: () => goTo("feedback") },
    { icon: "heart", label: t("settings.about.support"), onClick: () => goTo("support") },
  ];
  const recent = status?.recent ?? null;

  return (
    <>
      <header className="about-head">
        <AppMark size={72} />
        <div>
          <h1>{t("common.appName")}</h1>
          <p className="about-tagline">{t("settings.about.tagline")}</p>
          {info && <p className="about-version">{t("settings.about.version", { version: info.version })}</p>}
          {recent && (
            <p className="about-tagline">
              {recent.from
                ? t("settings.about.updatedFrom", { from: recent.from, date: formatDate(recent.at, lang) })
                : t("settings.about.updatedOn", { date: formatDate(recent.at, lang) })}
            </p>
          )}
        </div>
      </header>
      <p className="lead">{t("settings.about.description")}</p>
      <p className="promise">
        <Icon name="shield" size={16} />
        {t(privacyKeys(info).free)}
      </p>
      {recent && <WhatsNew t={t} lang={lang} version={recent.version} notes={recent.notes} />}
      <UpdateSection t={t} status={status} settings={settings} refreshStatus={refreshStatus} />
      <section className="group">
        <Row label={t("settings.about.autoCheck")} description={t("settings.about.autoCheckDesc")}>
          {({ labelId, descId }) => (
            <Switch
              labelledBy={labelId}
              describedBy={descId}
              checked={settings.autoCheckUpdates}
              onChange={(autoCheckUpdates) => update({ autoCheckUpdates })}
            />
          )}
        </Row>
        <Row label={t("settings.about.autoInstall")} description={t("settings.about.autoInstallDesc")}>
          {({ labelId, descId }) => (
            <Switch
              labelledBy={labelId}
              describedBy={descId}
              checked={settings.autoInstallUpdates}
              disabled={!settings.autoCheckUpdates}
              onChange={(autoInstallUpdates) => update({ autoInstallUpdates })}
            />
          )}
        </Row>
      </section>
      <nav className="about-links">
        {links.map((l) => (
          <button key={l.label} type="button" className="about-link" onClick={l.onClick}>
            <Icon name={l.icon} size={17} />
            <span>{l.label}</span>
            <Icon name="chevronRight" size={16} className="about-link-chevron" />
          </button>
        ))}
      </nav>
      {openFailed && <Notice tone="warning">{t("settings.support.openFailed")}</Notice>}
      <section className="about-legal">
        <p>{t("settings.about.license")}</p>
        <h2 className="section-title">{t("settings.about.creditsTitle")}</h2>
        <p>{t("settings.about.credits")}</p>
      </section>
    </>
  );
}

function WhatsNew({ t, lang, version, notes }: { t: TFunction; lang: Lang; version: string; notes: string | null }) {
  const text = pickNotes(notes, lang);
  if (!text) return null;
  return (
    <section className="group">
      <h2 className="section-title">{t("settings.about.whatsNew", { version })}</h2>
      <p className="lead" style={{ whiteSpace: "pre-wrap" }}>
        {text}
      </p>
      <Button size="s" variant="ghost" icon="x" onClick={() => api.hideUpdateNotes().catch(() => undefined)}>
        {t("settings.about.hideNotes")}
      </Button>
    </section>
  );
}

function UpdateSection({
  t,
  status,
  settings,
  refreshStatus,
}: {
  t: TFunction;
  status: UpdateStatus | null;
  settings: Settings;
  refreshStatus: () => void;
}) {
  const [local, setLocal] = useState<LocalUpdateState>("idle");
  // A snapshot set exists: the restart clears it, so the first click only says so.
  const [setWarning, setSetWarning] = useState(false);
  const view = updateView(status, local, settings);

  const check = async () => {
    setLocal("checking");
    try {
      const found = await api.checkUpdate();
      setLocal(found ? "idle" : "latest");
    } catch {
      setLocal("error");
    }
    refreshStatus();
  };

  const install = async () => {
    if (!setWarning) {
      const set = await api.getSet().catch(() => null);
      if (set && set.snapshots.length > 0) {
        setSetWarning(true);
        return;
      }
    }
    setLocal("installing");
    try {
      // Resolves only on failure: on success the app restarts into the new version.
      await api.installUpdate();
    } catch (e) {
      setLocal(stateAfterInstallError(e));
    }
    refreshStatus();
  };

  if (view.kind === "available" || view.kind === "installing") {
    const auto = view.kind === "available" && view.auto;
    return (
      <section className="about-update">
        <Notice tone="info" icon="refresh">
          {t(auto ? "settings.about.updateAuto" : "settings.about.updateAvailable", { version: view.version })}
        </Notice>
        {setWarning && view.kind === "available" && <Notice tone="warning">{t("settings.about.setWillClear")}</Notice>}
        <Button variant="primary" icon="refresh" busy={view.kind === "installing"} onClick={install}>
          {t("settings.about.installUpdate")}
        </Button>
        {view.kind === "available" && view.failed && <Notice tone="warning">{t("settings.about.updateFailed")}</Notice>}
        {view.kind === "available" && view.busy && <Notice tone="warning">{t("settings.about.updateBusy")}</Notice>}
      </section>
    );
  }
  return (
    <section className="about-update">
      <Button icon="refresh" busy={view.kind === "checking"} onClick={check}>
        {t("settings.about.checkUpdates")}
      </Button>
      {view.kind === "latest" && <span className="about-update-status">{t("settings.about.upToDate")}</span>}
      {view.kind === "error" && <Notice tone="warning">{t("settings.about.updateFailed")}</Notice>}
    </section>
  );
}
