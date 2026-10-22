import type { TFunction } from "../shared/i18n";

export const NUDGE_VARIANTS = 4;

/** Reminder copy: `index` is how many reminders were shown before (0..3); later ones reuse the last text. */
export function nudgeCopy(t: TFunction, index: number, captures: number): { title: string; body: string } {
  const variant = Math.min(Math.max(Math.trunc(index) || 0, 0), NUDGE_VARIANTS - 1);
  const shots = t("notice.shots", { count: Math.max(0, Math.trunc(captures) || 0) });
  return {
    title: t(`notice.nudge.v${variant}.title`, { shots }),
    body: t(`notice.nudge.v${variant}.body`, { shots }),
  };
}

/** The crash prompt after a crash in "ask" mode (analytics plan 3.4). */
export function crashCopy(t: TFunction) {
  return {
    title: t("notice.crash.title"),
    body: t("notice.crash.body"),
    send: t("notice.crash.send"),
    dontSend: t("notice.crash.dontSend"),
    always: t("notice.crash.always"),
    details: t("notice.crash.details"),
  };
}

export function fileName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const last = trimmed.split(/[\\/]/).pop();
  return last || path;
}

/** Toast detail is either a saved file or folder path (shown as its last segment) or a short message. */
export function toastDetail(detail: string | null): string | null {
  const text = detail?.trim();
  if (!text) return null;
  return /^([A-Za-z]:[\\/]|\\\\|\/|~\/)/.test(text) ? fileName(text) : text;
}
