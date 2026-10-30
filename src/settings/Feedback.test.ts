import { describe, expect, it } from "vitest";
import { createT } from "../shared/i18n";
import { FEEDBACK_ERRORS, isFeedbackError } from "./Feedback";

describe("feedback", () => {
  it("recognises the core's error codes only", () => {
    for (const code of FEEDBACK_ERRORS) expect(isFeedbackError(code)).toBe(true);
    for (const other of ["rate_limited", "", "Failed", null, undefined, 3, { error: "failed" }])
      expect(isFeedbackError(other)).toBe(false);
  });

  it("has a text for every error and the form in both languages", () => {
    for (const lang of ["ru", "en"] as const) {
      const t = createT(lang);
      const keys = [
        ...FEEDBACK_ERRORS.map((code) => `settings.feedback.error.${code}`),
        "settings.feedback.title",
        "settings.feedback.send",
        "settings.feedback.sent",
        "settings.feedback.writeDirect",
        "settings.about.feedback",
        "settings.support.feedback",
      ];
      for (const key of keys) expect(t(key), key).not.toBe(key);
      expect(t("settings.feedback.attached", { version: "0.3.2", os: "Windows 11 (26100)", lang: "ru" })).toContain("Windows 11 (26100)");
    }
  });
});
