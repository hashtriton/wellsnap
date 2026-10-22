import type { common as ru } from "../ru/common";

export const common: typeof ru = {
  appName: "WellSnap",
  snapshots: { one: "{count} screenshot", few: "{count} screenshots", many: "{count} screenshots", other: "{count} screenshots" },
  comments: { one: "{count} comment", few: "{count} comments", many: "{count} comments", other: "{count} comments" },
  actions: {
    copy: "Copy",
    save: "Save",
    cancel: "Cancel",
    close: "Close",
    done: "Done",
    undo: "Undo",
    redo: "Redo",
    retry: "Try again",
    openFolder: "Show in folder",
  },
};
