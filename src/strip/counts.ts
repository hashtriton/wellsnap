import type { Snapshot } from "../shared/ipc";

/** Marks with a comment; numbered marks without text are not counted. */
export function commentCount(snap: Pick<Snapshot, "marks">): number {
  return snap.marks.filter((m) => m.comment.trim() !== "").length;
}

export function totalComments(snaps: Pick<Snapshot, "marks">[]): number {
  return snaps.reduce((sum, s) => sum + commentCount(s), 0);
}
