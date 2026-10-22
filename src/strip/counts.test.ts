import { describe, expect, it } from "vitest";
import { createT } from "../shared/i18n";
import { commentCount, totalComments } from "./counts";

const snap = (...comments: string[]) => ({ marks: comments.map((comment, i) => ({ num: i + 1, comment })) });

describe("strip counts", () => {
  it("counts only marks with text", () => {
    expect(commentCount(snap("a", "", "  ", "b"))).toBe(2);
    expect(commentCount(snap())).toBe(0);
    expect(totalComments([snap("a"), snap("", "b", "c"), snap()])).toBe(3);
    expect(totalComments([])).toBe(0);
  });

  it("renders the header with plurals", () => {
    const t = createT("ru");
    expect(`${t("common.snapshots", { count: 3 })}, ${t("common.comments", { count: 4 })}`).toBe("3 снимка, 4 комментария");
    expect(t("common.comments", { count: 5 })).toBe("5 комментариев");
  });
});
