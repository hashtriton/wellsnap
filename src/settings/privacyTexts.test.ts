import { describe, expect, it } from "vitest";
import { createT } from "../shared/i18n";
import type { AppInfo } from "../shared/ipc";
import { privacyKeys } from "./privacyTexts";

const info = (telemetry: boolean) => ({ telemetry }) as AppInfo;

describe("privacyKeys", () => {
  for (const lang of ["ru", "en"] as const) {
    const t = createT(lang);
    const texts = (i: AppInfo | null) => Object.values(privacyKeys(i)).map((key) => [key, t(key, { shots: "1", marks: "2" })]);

    it(`names the services only in a build that sends (${lang})`, () => {
      for (const [key, text] of texts(info(true))) expect(text, key).not.toBe(key);
      expect(t(privacyKeys(info(true)).privacy)).toMatch(/PostHog.*Sentry/);
      for (const i of [info(false), null]) {
        for (const [key, text] of texts(i)) {
          expect(text, key).not.toBe(key);
          expect(text, key).not.toMatch(/PostHog|Sentry/);
        }
      }
    });
  }
});
