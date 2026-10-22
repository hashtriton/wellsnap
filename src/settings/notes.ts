// Release notes and the update block of About, kept pure for tests.
import type { Lang, UpdateStatus } from "../shared/ipc";

/** Release notes (release-notes/vX.Y.Z.md) have a "### Русский" and a "### English" section. */
const SECTIONS: Record<Lang, RegExp> = { ru: /^русский$/i, en: /^english$/i };
const HEADING = /^#{1,3}\s+(.*?)\s*#*\s*$/;

const HTML_COMMENT = /<!--[\s\S]*?-->/g;

/**
 * The notes in the UI language: its own section, else the other language's, else the whole text when the
 * notes have no such sections. Text outside the two sections (a header, a footer) and HTML comments are left
 * out. notes_have_text in src-tauri/src/updates/policy.rs reads them the same way.
 */
export function pickNotes(notes: string | null | undefined, lang: Lang): string | null {
  const text = notes?.replace(HTML_COMMENT, "").trim();
  if (!text) return null;
  const sections: Partial<Record<Lang, string[]>> = {};
  let current: string[] | null = null;
  for (const line of text.split(/\r?\n/)) {
    const heading = HEADING.exec(line);
    if (heading) {
      const title = heading[1];
      const found = (Object.keys(SECTIONS) as Lang[]).find((l) => SECTIONS[l].test(title));
      current = found ? (sections[found] = []) : null;
      continue;
    }
    current?.push(line);
  }
  if (!sections.ru && !sections.en) return text;
  const body = (l: Lang) => sections[l]?.join("\n").trim() || null;
  return body(lang) ?? body(lang === "ru" ? "en" : "ru");
}

/** What the About page itself is doing; the core's phase wins once it knows the version. */
export type LocalUpdateState = "idle" | "checking" | "latest" | "error" | "busy" | "installing";

export type UpdateView =
  | { kind: "idle" | "checking" | "latest" | "error" }
  | { kind: "available"; version: string; auto: boolean; failed: boolean; busy: boolean }
  | { kind: "installing"; version: string };

export function updateView(
  status: UpdateStatus | null,
  local: LocalUpdateState,
  settings: { autoCheckUpdates: boolean; autoInstallUpdates: boolean },
): UpdateView {
  const version = status?.info?.version;
  if (status && version && status.phase !== "idle") {
    if (status.phase === "installing" || local === "installing") return { kind: "installing", version };
    const auto = status.autoEligible && settings.autoCheckUpdates && settings.autoInstallUpdates;
    return { kind: "available", version, auto, failed: local === "error", busy: local === "busy" };
  }
  if (local === "installing") return { kind: "checking" };
  return { kind: local === "busy" ? "idle" : local };
}

/**
 * After "Update and restart" was refused: "busy" (a capture, a paste or another install is running) and
 * "gone" (the release was pulled meanwhile) are the codes of src-tauri/src/updates/mod.rs.
 */
export function stateAfterInstallError(error: unknown): LocalUpdateState {
  if (error === "busy") return "busy";
  if (error === "gone") return "latest";
  return "error";
}

/** "21 October 2026" / "21 октября 2026 г." from Unix seconds. */
export function formatDate(unixSeconds: number, lang: Lang): string {
  return new Intl.DateTimeFormat(lang, { day: "numeric", month: "long", year: "numeric" }).format(
    new Date(unixSeconds * 1000),
  );
}
