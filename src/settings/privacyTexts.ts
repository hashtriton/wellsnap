// What WellSnap says it sends must hold for this build: forks and local builds have no telemetry keys
// (AppInfo.telemetry false) and send nothing, so they keep the "never sent" wording.
import type { AppInfo } from "../shared/ipc";

export function privacyKeys(info: AppInfo | null) {
  return info?.telemetry
    ? { privacy: "settings.support.privacy", stats: "settings.support.stats", free: "settings.about.free" }
    : { privacy: "settings.support.privacyLocal", stats: "settings.support.statsLocal", free: "settings.about.freeLocal" };
}
