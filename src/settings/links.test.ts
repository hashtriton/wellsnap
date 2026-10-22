import { describe, expect, it } from "vitest";
import { isAllowedExternalUrl, REPO_URL } from "../shared/donate";
import { pasteTargetsUrl } from "./links";

describe("pasteTargetsUrl", () => {
  it("opens the paste matrix in the UI language", () => {
    expect(pasteTargetsUrl("ru")).toBe(`${REPO_URL}/blob/main/docs/paste-targets.ru.md`);
    expect(pasteTargetsUrl("en")).toBe(`${REPO_URL}/blob/main/docs/paste-targets.md`);
  });

  it("passes the external link allowlist", () => {
    expect(isAllowedExternalUrl(pasteTargetsUrl("ru"))).toBe(true);
    expect(isAllowedExternalUrl(pasteTargetsUrl("en"))).toBe(true);
  });
});
