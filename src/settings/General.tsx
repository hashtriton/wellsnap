import type { LanguagePref, Theme } from "../shared/ipc";
import { Row, Segmented, Switch } from "../shared/ui/controls";
import type { PageProps } from "./types";

export function General({ t, platform, settings, update }: PageProps) {
  return (
    <>
      <PageHead title={t("settings.general.title")} lead={t("settings.general.lead")} />
      <section className="group">
        <Row label={t("settings.general.language")}>
          {({ labelId }) => (
            <Segmented<LanguagePref>
              labelledBy={labelId}
              value={settings.language}
              onChange={(language) => update({ language })}
              options={[
                { value: "auto", label: t("settings.general.languageAuto") },
                // Language names are always shown in their own language.
                { value: "ru", label: "Русский" },
                { value: "en", label: "English" },
              ]}
            />
          )}
        </Row>
        <Row label={t("settings.general.themeLabel")}>
          {({ labelId }) => (
            <Segmented<Theme>
              labelledBy={labelId}
              value={settings.theme}
              onChange={(theme) => update({ theme })}
              options={[
                { value: "system", label: t("settings.general.themeSystem") },
                { value: "light", label: t("settings.general.themeLight") },
                { value: "dark", label: t("settings.general.themeDark") },
              ]}
            />
          )}
        </Row>
      </section>
      <section className="group">
        <Row
          label={t("settings.general.autostart")}
          description={t(platform === "macos" ? "settings.general.autostartDescMac" : "settings.general.autostartDesc")}
        >
          {({ labelId, descId }) => (
            <Switch
              labelledBy={labelId}
              describedBy={descId}
              checked={settings.autostart}
              onChange={(autostart) => update({ autostart })}
            />
          )}
        </Row>
        <Row label={t("settings.general.hints")} description={t("settings.general.hintsDesc")}>
          {({ labelId, descId }) => (
            <Switch
              labelledBy={labelId}
              describedBy={descId}
              checked={settings.showHints}
              onChange={(showHints) => update({ showHints })}
            />
          )}
        </Row>
      </section>
    </>
  );
}

export function PageHead({ title, lead }: { title: string; lead?: string }) {
  return (
    <header className="page-head">
      <h1>{title}</h1>
      {lead && <p>{lead}</p>}
    </header>
  );
}
