// Payment methods live on the site's donation page, so they change with a site deploy, not an app release.
// External links must pass the allowlist of `open_external` (src-tauri/src/commands.rs).
import type { Lang } from "./ipc";

export const REPO_URL = "https://github.com/hashtriton/wellsnap";
export const RELEASES_URL = `${REPO_URL}/releases`;

/** Donation page in the app language; `from=app` tells visits from the app apart in the site's statistics. */
export function donatePageUrl(lang: Lang): string {
  return `https://wellsnap.net${lang === "ru" ? "/ru" : ""}/donate/?from=app`;
}

/** Mirror of EXTERNAL_HOSTS in src-tauri/src/commands.rs. */
const EXTERNAL_HOSTS = ["github.com", "wellsnap.net"];

/**
 * Same rule as the Rust side: https only, no credentials or port, host on the allowlist or one of its
 * subdomains. The host is taken from the parsed URL, as the browser will see it: a backslash counts as "/"
 * there, so "https://evil.com\.github.com" opens evil.com and is rejected outright.
 */
export function isAllowedExternalUrl(url: string): boolean {
  if (url.includes("\\")) return false;
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.protocol !== "https:" || u.username || u.password || u.port) return false;
  const host = u.hostname;
  return host !== "" && EXTERNAL_HOSTS.some((h) => host === h || host.endsWith(`.${h}`));
}
