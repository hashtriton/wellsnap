// Donation links and addresses. Free forever: nothing in the app depends on these values, empty ones are
// simply not shown. External links must pass the allowlist of `open_external` (src-tauri/src/commands.rs).
import type { Lang } from "./ipc";

/** CloudTips personal page (Russian bank cards), e.g. "https://pay.cloudtips.ru/p/xxxxxxxx". */
export const CLOUDTIPS_URL: string = "";
/** TON wallet address for TON coins. */
export const TON_ADDRESS: string = "";
/** TON wallet address that receives USDT (jetton on the TON network). */
export const USDT_TON_ADDRESS: string = "";

export const REPO_URL = "https://github.com/hashtriton/wellsnap";
export const RELEASES_URL = `${REPO_URL}/releases`;
export const ISSUES_URL = `${REPO_URL}/issues`;

/** Mirror of EXTERNAL_HOSTS in src-tauri/src/commands.rs. */
const EXTERNAL_HOSTS = ["github.com", "pay.cloudtips.ru", "cloudtips.ru", "wellsnap.app", "tonviewer.com"];

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

/** User-friendly TON address (48 base64url chars) or raw form "0:<64 hex>". */
export function isTonAddress(address: string): boolean {
  return /^[A-Za-z0-9_-]{48}$/.test(address) || /^-?[0-9]+:[0-9a-fA-F]{64}$/.test(address);
}

export interface DonationConfig {
  cloudtips: string;
  ton: string;
  usdt: string;
}

export const DONATION_CONFIG: DonationConfig = {
  cloudtips: CLOUDTIPS_URL,
  ton: TON_ADDRESS,
  usdt: USDT_TON_ADDRESS,
};

export type PaymentMethod =
  | { id: "cloudtips"; url: string }
  | { id: "crypto"; ton: string | null; usdt: string | null };

/** Configured methods in display order: CloudTips first for Russian (Russian cards only), crypto first otherwise. */
export function paymentMethods(lang: Lang, config: DonationConfig = DONATION_CONFIG): PaymentMethod[] {
  const methods: PaymentMethod[] = [];
  const url = config.cloudtips.trim();
  if (url && isAllowedExternalUrl(url)) methods.push({ id: "cloudtips", url });
  const ton = config.ton.trim();
  const usdt = config.usdt.trim();
  const crypto = {
    id: "crypto" as const,
    ton: ton && isTonAddress(ton) ? ton : null,
    usdt: usdt && isTonAddress(usdt) ? usdt : null,
  };
  if (crypto.ton || crypto.usdt) {
    if (lang === "ru") methods.push(crypto);
    else methods.unshift(crypto);
  }
  return methods;
}

/** False while the build ships no donation method: then the reminder card has nothing to offer. */
export function hasPaymentMethods(config: DonationConfig = DONATION_CONFIG): boolean {
  return paymentMethods("en", config).length > 0;
}

export type SupportTarget ={ kind: "url"; url: string } | { kind: "settings" };

/** Where "Support" on the reminder card leads: the first method that is a plain link, else the Support tab. */
export function supportTarget(lang: Lang, config: DonationConfig = DONATION_CONFIG): SupportTarget {
  const first = paymentMethods(lang, config)[0];
  return first?.id === "cloudtips" ? { kind: "url", url: first.url } : { kind: "settings" };
}
