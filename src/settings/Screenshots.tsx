import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import type { CommentPlacement, EnterAction, ImageFormat } from "../shared/ipc";
import { Button, Row, Segmented, Switch } from "../shared/ui/controls";
import { formatAccelerator } from "../shared/ui/hotkey";
import { Icon } from "../shared/ui/icons";
import { debouncedCommit } from "./debounce";
import { PageHead } from "./General";
import type { PageProps } from "./types";

const QUALITY_MIN = 40;
const QUALITY_MAX = 100;

export function Screenshots({ t, platform, settings, info, update, refreshInfo }: PageProps) {
  const [picking, setPicking] = useState(false);
  const folder = settings.saveDir ?? info?.saveDir ?? "";
  const pasteHotkey = formatAccelerator(settings.smartPasteHotkey, platform);
  const placementDesc =
    settings.commentPlacement === "onImage"
      ? t("settings.screenshots.placementOnImageDesc")
      : pasteHotkey
        ? t("settings.screenshots.placementSeparateDesc", { hotkey: pasteHotkey })
        : t("settings.screenshots.placementSeparateDescNoKey");

  const pickFolder = async () => {
    setPicking(true);
    try {
      const picked = await open({ directory: true, defaultPath: folder || undefined, title: t("settings.screenshots.chooseFolder") });
      if (typeof picked === "string" && picked) {
        await update({ saveDir: picked });
        refreshInfo();
      }
    } catch {
      // The dialog failing to open leaves the setting as it was.
    } finally {
      setPicking(false);
    }
  };

  const useDefault = async () => {
    await update({ saveDir: null });
    refreshInfo();
  };

  return (
    <>
      <PageHead title={t("settings.screenshots.title")} lead={t("settings.screenshots.lead")} />
      <section className="group">
        <Row label={t("settings.screenshots.folder")} stacked>
          {({ labelId }) => (
            <div className="folder">
              <div className="folder-path" aria-labelledby={labelId} title={folder}>
                <Icon name="folder" size={16} />
                <span>{settings.saveDir ? folder : `${t("settings.screenshots.folderDefault")}${folder ? `: ${folder}` : ""}`}</span>
              </div>
              <div className="folder-actions">
                <Button size="s" busy={picking} onClick={pickFolder}>
                  {t("settings.screenshots.change")}
                </Button>
                {settings.saveDir && (
                  <Button size="s" variant="ghost" icon="reset" onClick={useDefault}>
                    {t("settings.screenshots.useDefault")}
                  </Button>
                )}
              </div>
            </div>
          )}
        </Row>
        <Row
          label={t("settings.screenshots.format")}
          description={settings.imageFormat === "jpg" ? t("settings.screenshots.formatJpgDesc") : t("settings.screenshots.formatPngDesc")}
        >
          {({ labelId }) => (
            <Segmented<ImageFormat>
              labelledBy={labelId}
              value={settings.imageFormat}
              onChange={(imageFormat) => update({ imageFormat })}
              options={[
                { value: "png", label: "PNG" },
                { value: "jpg", label: "JPG" },
              ]}
            />
          )}
        </Row>
        {settings.imageFormat === "jpg" && (
          <Row label={t("settings.screenshots.quality")}>
            {({ labelId }) => <QualitySlider labelledBy={labelId} value={settings.jpgQuality} t={t} onCommit={(jpgQuality) => update({ jpgQuality })} />}
          </Row>
        )}
        <Row label={t("settings.screenshots.enter")}>
          {({ labelId }) => (
            <Segmented<EnterAction>
              labelledBy={labelId}
              value={settings.enterAction}
              onChange={(enterAction) => update({ enterAction })}
              options={[
                { value: "copy", label: t("settings.screenshots.enterCopy") },
                { value: "save", label: t("settings.screenshots.enterSave") },
              ]}
            />
          )}
        </Row>
      </section>
      <section className="group">
        <Row label={t("settings.screenshots.placement")} description={placementDesc}>
          {({ labelId }) => (
            <Segmented<CommentPlacement>
              labelledBy={labelId}
              value={settings.commentPlacement}
              onChange={(commentPlacement) => update({ commentPlacement })}
              options={[
                { value: "onImage", label: t("settings.screenshots.placementOnImage") },
                { value: "separate", label: t("settings.screenshots.placementSeparate") },
              ]}
            />
          )}
        </Row>
        <Row label={t("settings.screenshots.hideStrip")} description={t("settings.screenshots.hideStripDesc")}>
          {({ labelId, descId }) => (
            <Switch
              labelledBy={labelId}
              describedBy={descId}
              checked={settings.hideStripFromCapture}
              onChange={(hideStripFromCapture) => update({ hideStripFromCapture })}
            />
          )}
        </Row>
      </section>
    </>
  );
}

/** Saves when the drag ends or the value settles, not on every pixel of the drag. */
function QualitySlider({
  value,
  labelledBy,
  t,
  onCommit,
}: {
  value: number;
  labelledBy: string;
  t: PageProps["t"];
  onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState(value);
  const commitRef = useRef(onCommit);
  commitRef.current = onCommit;
  const [pending] = useState(() => debouncedCommit<number>(300, (next) => commitRef.current(next)));
  useEffect(() => setDraft(value), [value]);
  // A change made right before switching tabs is saved, not dropped with the timer.
  useEffect(() => () => pending.flush(), [pending]);

  const change = (next: number) => {
    setDraft(next);
    pending.push(next);
  };

  const percent = ((draft - QUALITY_MIN) / (QUALITY_MAX - QUALITY_MIN)) * 100;
  return (
    <div className="quality">
      <input
        type="range"
        min={QUALITY_MIN}
        max={QUALITY_MAX}
        step={5}
        value={draft}
        aria-labelledby={labelledBy}
        aria-valuetext={t("settings.screenshots.qualityValue", { value: draft })}
        style={{ ["--fill" as string]: `${percent}%` }}
        onChange={(e) => change(Number(e.target.value))}
        onPointerUp={pending.flush}
        onKeyUp={pending.flush}
      />
      <output className="quality-value">{t("settings.screenshots.qualityValue", { value: draft })}</output>
    </div>
  );
}
