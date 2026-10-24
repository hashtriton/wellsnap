// "Support" section (donations research, sections 6-8): honest ask, nothing gated, local stats only.
// Payment methods themselves are on the site's donation page.
import { useState } from "react";
import { api } from "../shared/ipc";
import type { TFunction } from "../shared/i18n";
import { donatePageUrl, ISSUES_URL, REPO_URL } from "../shared/donate";
import { Button, Notice, Row, Segmented, Switch } from "../shared/ui/controls";
import { Icon } from "../shared/ui/icons";
import { QrCode } from "../shared/ui/QrCode";
import { PageHead } from "./General";
import { openLink } from "./links";
import { privacyKeys } from "./privacyTexts";
import type { PageProps } from "./types";

type Reminders = "rare" | "never";

export function Support({ t, lang, settings, info }: PageProps) {
  const d = settings.donations;
  const texts = privacyKeys(info);
  const supporter = d.status === "supporter";
  const pageUrl = donatePageUrl(lang);
  const [openFailed, setOpenFailed] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);

  const open = async (url: string) => setOpenFailed(!(await openLink(url)));
  const setStatus = (nextSupporter: boolean, reminders: boolean) =>
    api.donationSetStatus(nextSupporter, reminders).catch(() => undefined);

  return (
    <>
      <PageHead title={t("settings.support.title")} />
      {supporter && (
        <div className="thanks">
          <Icon name="heart" size={16} />
          {t("settings.support.thanks")}
        </div>
      )}
      <p className="lead">{t("settings.support.lead")}</p>
      <p className="promise">
        <Icon name="shield" size={16} />
        {t("settings.support.promise")}
      </p>

      <section className="group">
        <Row
          label={t("settings.support.statsTitle")}
          description={t(texts.stats, {
            shots: t("settings.support.shots", { count: d.captures }),
            marks: t("settings.support.marks", { count: d.marks }),
          })}
        >
          {() =>
            confirmReset ? (
              <div className="confirm" role="group" aria-label={t("settings.support.resetConfirm")}>
                <span>{t("settings.support.resetConfirm")}</span>
                <Button
                  size="s"
                  variant="danger"
                  autoFocus
                  onClick={() => {
                    setConfirmReset(false);
                    api.donationResetCounters().catch(() => undefined);
                  }}
                >
                  {t("settings.support.resetYes")}
                </Button>
                <Button size="s" variant="ghost" onClick={() => setConfirmReset(false)}>
                  {t("settings.support.resetNo")}
                </Button>
              </div>
            ) : (
              <Button size="s" icon="reset" disabled={d.captures === 0 && d.marks === 0} onClick={() => setConfirmReset(true)}>
                {t("settings.support.resetCounters")}
              </Button>
            )
          }
        </Row>
      </section>

      <section className="methods" aria-label={t("settings.support.pageTitle")}>
        <div className="card method">
          <div className="method-main">
            <div className="card-head">
              <span className="card-icon">
                <Icon name="heart" />
              </span>
              <h3 className="card-title">{t("settings.support.pageTitle")}</h3>
            </div>
            <p className="card-text">{t("settings.support.pageDesc")}</p>
            <div className="card-actions">
              <Button variant="primary" icon="external" onClick={() => open(pageUrl)}>
                {t("settings.support.pageOpen")}
              </Button>
            </div>
          </div>
          <QrToggle text={pageUrl} label={t("settings.support.qrPage")} t={t} />
        </div>
        {openFailed && <Notice tone="warning">{t("settings.support.openFailed")}</Notice>}
      </section>

      <section className="group">
        <Row label={t("settings.support.supporter")} description={t("settings.support.supporterDesc")}>
          {({ labelId, descId }) => (
            <Switch labelledBy={labelId} describedBy={descId} checked={supporter} onChange={(on) => setStatus(on, true)} />
          )}
        </Row>
        <Row
          label={t("settings.support.reminders")}
          description={supporter ? t("settings.support.remindersOffSupporter") : t("settings.support.remindersDesc")}
        >
          {({ labelId }) => (
            <Segmented<Reminders>
              labelledBy={labelId}
              disabled={supporter}
              value={supporter || d.status === "never" ? "never" : "rare"}
              onChange={(mode) => setStatus(false, mode === "rare")}
              options={[
                { value: "rare", label: t("settings.support.remindersRare") },
                { value: "never", label: t("settings.support.remindersNever") },
              ]}
            />
          )}
        </Row>
      </section>

      <section className="info-blocks">
        <div>
          <h2 className="section-title">{t("settings.support.moneyTitle")}</h2>
          <p>{t("settings.support.money")}</p>
        </div>
        <div>
          <h2 className="section-title">{t("settings.support.helpTitle")}</h2>
          <p>{t("settings.support.help")}</p>
          <div className="card-actions">
            <Button size="s" icon="star" onClick={() => open(REPO_URL)}>
              {t("settings.support.star")}
            </Button>
            <Button size="s" icon="message" onClick={() => open(ISSUES_URL)}>
              {t("settings.support.issue")}
            </Button>
          </div>
        </div>
      </section>
      <p className="footnote">
        <Icon name="shield" size={14} />
        {t(texts.privacy)}
      </p>
    </>
  );
}

/** `label` is the full accessible name of the QR code. */
function QrToggle({ text, label, t }: { text: string; label: string; t: TFunction }) {
  const [shown, setShown] = useState(false);
  return (
    <div className="qr-box">
      {shown ? (
        <>
          <QrCode text={text} size={132} label={label} />
          <span className="qr-hint">{t("settings.support.qrHint")}</span>
          <button type="button" className="link-btn" onClick={() => setShown(false)}>
            {t("settings.support.hideQr")}
          </button>
        </>
      ) : (
        <Button size="s" variant="ghost" icon="qr" onClick={() => setShown(true)}>
          {t("settings.support.showQr")}
        </Button>
      )}
    </div>
  );
}
