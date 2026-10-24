import { describe, expect, it } from "vitest";
import { donatePageUrl, isAllowedExternalUrl, ISSUES_URL, RELEASES_URL, REPO_URL } from "./donate";

describe("donate", () => {
  it("mirrors the Rust allowlist", () => {
    expect(isAllowedExternalUrl(REPO_URL)).toBe(true);
    expect(isAllowedExternalUrl(RELEASES_URL)).toBe(true);
    expect(isAllowedExternalUrl(ISSUES_URL)).toBe(true);
    expect(isAllowedExternalUrl("https://wellsnap.net/ru/donate/?from=app")).toBe(true);
    expect(isAllowedExternalUrl("https://www.wellsnap.net/")).toBe(true);
    expect(isAllowedExternalUrl("https://pay.cloudtips.ru/p/12345678")).toBe(false);
    expect(isAllowedExternalUrl("https://wellsnap.app/")).toBe(false);
    expect(isAllowedExternalUrl("http://github.com")).toBe(false);
    expect(isAllowedExternalUrl("https://github.com.evil.io/")).toBe(false);
    expect(isAllowedExternalUrl("https://evilgithub.com/")).toBe(false);
    expect(isAllowedExternalUrl("https://wellsnap.net.evil.io/")).toBe(false);
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

  it("links to the donation page in the app language", () => {
    expect(donatePageUrl("ru")).toBe("https://wellsnap.net/ru/donate/?from=app");
    expect(donatePageUrl("en")).toBe("https://wellsnap.net/donate/?from=app");
    expect(isAllowedExternalUrl(donatePageUrl("ru"))).toBe(true);
    expect(isAllowedExternalUrl(donatePageUrl("en"))).toBe(true);
  });
});
