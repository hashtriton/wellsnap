// Shared strings. Every area keeps its own file (overlay.ts, strip.ts, ...) to avoid edit conflicts.
// No long or en dashes in UI text (owner's rule): use a hyphen, colon or comma instead.
export const common = {
  appName: "WellSnap",
  snapshots: { one: "{count} снимок", few: "{count} снимка", many: "{count} снимков", other: "{count} снимка" },
  comments: {
    one: "{count} комментарий",
    few: "{count} комментария",
    many: "{count} комментариев",
    other: "{count} комментария",
  },
  actions: {
    copy: "Копировать",
    save: "Сохранить",
    cancel: "Отмена",
    close: "Закрыть",
    done: "Готово",
    undo: "Отменить",
    redo: "Повторить",
    retry: "Повторить попытку",
    openFolder: "Показать в папке",
  },
};
