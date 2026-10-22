import type { TFunction } from "../shared/i18n";
import type { AppInfo, Lang, Platform, Settings } from "../shared/ipc";
import type { SettingsPatch } from "../shared/ui/hooks";
import type { Tab } from "./tabs";

/** What every settings page gets from the window. */
export interface PageProps {
  t: TFunction;
  lang: Lang;
  platform: Platform;
  settings: Settings;
  info: AppInfo | null;
  /** Applies a patch right away; on failure the window shows an error and the controls revert. */
  update: (patch: SettingsPatch) => Promise<boolean>;
  refreshInfo: () => void;
  goTo: (tab: Tab) => void;
}
