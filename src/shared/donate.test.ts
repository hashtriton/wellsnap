import { describe, expect, it } from "vitest";
import {
  DONATION_CONFIG,
  hasPaymentMethods,
  isAllowedExternalUrl,
  isTonAddress,
  paymentMethods,
  RELEASES_URL,
  REPO_URL,
  supportTarget,
} from "./donate";

const TON = "UQ" + "A".repeat(20) + "b-_c" + "1".repeat(22);
const USDT = "EQ" + "z".repeat(46);
const CT = "https://pay.cloudtips.ru/p/12345678";
const full = { cloudtips: CT, ton: TON, usdt: USDT };

describe("donate", () => {
  it("mirrors the Rust allowlist", () => {
    expect(isAllowedExternalUrl(CT)).toBe(true);
    expect(isAllowedExternalUrl(REPO_URL)).toBe(true);
    expect(isAllowedExternalUrl(RELEASES_URL)).toBe(true);
    expect(isAllowedExternalUrl("https://tonviewer.com/x?y=1")).toBe(true);
    expect(isAllowedExternalUrl("https://sub.wellsnap.app")).toBe(true);
    expect(isAllowedExternalUrl("http://github.com")).toBe(false);
    expect(isAllowedExternalUrl("https://github.com.evil.io/")).toBe(false);
    expect(isAllowedExternalUrl("https://evilgithub.com/")).toBe(false);
    expect(isAllowedExternalUrl("https://")).toBe(false);
  });

  it("checks the host the browser will open", () => {
    expect(isAllowedExternalUrl("https://sub.github.com/x")).toBe(true);
    expect(isAllowedExternalUrl("https://GitHub.com/x")).toBe(true);
    // Browsers read a backslash as "/", so these open evil.com and evil.example.
    expect(isAllowedExternalUrl("https://evil.com\\.github.com/")).toBe(false);
    expect(isAllowedExternalUrl("https://evil.example\\.github.com/x")).toBe(false);
    expect(new URL("https://evil.example\\.github.com/x").hostname).toBe("evil.example");
    expect(isAllowedExternalUrl("https://evil.com@github.com")).toBe(false);
    expect(isAllowedExternalUrl("https://github.com@evil.com/")).toBe(false);
    expect(isAllowedExternalUrl("https://user@github.com/")).toBe(false);
    expect(isAllowedExternalUrl("https://github.com:8443")).toBe(false);
    expect(isAllowedExternalUrl("not a url")).toBe(false);
  });

  it("validates TON addresses", () => {
    expect(TON).toHaveLength(48);
    expect(isTonAddress(TON)).toBe(true);
    expect(isTonAddress("0:" + "a".repeat(64))).toBe(true);
    expect(isTonAddress("EQshort")).toBe(false);
    expect(isTonAddress("0x" + "a".repeat(40))).toBe(false);
    expect(isTonAddress("")).toBe(false);
  });

  it("orders methods by language and hides empty ones", () => {
    expect(paymentMethods("ru", full).map((m) => m.id)).toEqual(["cloudtips", "crypto"]);
    expect(paymentMethods("en", full).map((m) => m.id)).toEqual(["crypto", "cloudtips"]);
    expect(paymentMethods("ru", { cloudtips: "", ton: "", usdt: "" })).toEqual([]);
    expect(paymentMethods("en", { cloudtips: "", ton: "", usdt: USDT })).toEqual([{ id: "crypto", ton: null, usdt: USDT }]);
  });

  it("drops values that would not work", () => {
    const bad = { cloudtips: "https://example.com/pay", ton: "nope", usdt: "  " };
    expect(paymentMethods("ru", bad)).toEqual([]);
  });

  it("knows whether any method is configured", () => {
    expect(hasPaymentMethods(full)).toBe(true);
    expect(hasPaymentMethods({ cloudtips: "", ton: "", usdt: USDT })).toBe(true);
    expect(hasPaymentMethods({ cloudtips: "", ton: "", usdt: "" })).toBe(false);
    expect(hasPaymentMethods({ cloudtips: "https://example.com/pay", ton: "nope", usdt: "" })).toBe(false);
  });

  it("picks the support target", () => {
    expect(supportTarget("ru", full)).toEqual({ kind: "url", url: CT });
    expect(supportTarget("en", full)).toEqual({ kind: "settings" });
    expect(supportTarget("en", { cloudtips: CT, ton: "", usdt: "" })).toEqual({ kind: "url", url: CT });
    expect(supportTarget("ru", { cloudtips: "", ton: TON, usdt: "" })).toEqual({ kind: "settings" });
  });

  it("ships values that are either empty or valid", () => {
    const { cloudtips, ton, usdt } = DONATION_CONFIG;
    expect(cloudtips === "" || isAllowedExternalUrl(cloudtips)).toBe(true);
    expect(ton === "" || isTonAddress(ton)).toBe(true);
    expect(usdt === "" || isTonAddress(usdt)).toBe(true);
  });
});
