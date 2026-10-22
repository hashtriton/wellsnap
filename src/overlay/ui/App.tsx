import { useLayoutEffect, useRef, useSyncExternalStore, type FocusEvent, type KeyboardEvent } from "react";
import { useI18n, type TFunction } from "../../shared/i18n";
import { useThemeSync } from "../../shared/theme";
import type { Editor, InputView, View } from "../editor";
import { formatBinding, inputKeyAction, KEYMAP, type CommandId } from "../keys";
import { contrastText } from "../marks";
import { Floating } from "./Floating";
import { CloseIcon } from "./Icons";
import { Toolbar } from "./Toolbar";

const MAX_COMMENT = 500;

export function App({ editor }: { editor: Editor }) {
  const view = useSyncExternalStore(editor.subscribe, editor.getView);
  const { t } = useI18n();
  useThemeSync();

  if (view.phase === "empty") return null;
  return (
    <>
      {view.phase === "idle" && view.showHint && <div className="hint glass">{t("overlay.hint")}</div>}
      {view.settled && view.selection && <Toolbar editor={editor} view={view} t={t} />}
      {view.input?.kind === "mark" && <MarkInput editor={editor} input={view.input} view={view} t={t} />}
      {view.input?.kind === "text" && <TextInput editor={editor} input={view.input} view={view} t={t} />}
      {view.armed ? (
        <div className="plate glass" role="status">
          {t("overlay.escConfirm")}
        </div>
      ) : (
        view.notice && (
          <div className={`plate glass${view.notice.tone === "error" ? " error" : ""}`} role={view.notice.tone === "error" ? "alert" : "status"}>
            {t(view.notice.key, view.notice.params)}
            {view.notice.detail && <span className="plate-detail">{view.notice.detail}</span>}
          </div>
        )
      )}
      {view.busy && (
        <span className="sr-only" role="status">
          {t("overlay.busy")}
        </span>
      )}
      {view.help && <HelpPanel editor={editor} view={view} t={t} />}
    </>
  );
}

function inputKeys(editor: Editor, mac: boolean) {
  return (e: KeyboardEvent<HTMLInputElement>) => {
    const el = e.currentTarget;
    const action = inputKeyAction(
      {
        code: e.code,
        key: e.key,
        ctrlKey: e.ctrlKey,
        metaKey: e.metaKey,
        shiftKey: e.shiftKey,
        altKey: e.altKey,
        repeat: e.repeat,
        isComposing: e.nativeEvent.isComposing,
        keyCode: e.nativeEvent.keyCode,
      },
      mac,
      el.selectionStart !== el.selectionEnd,
    );
    if (!action) return;
    e.preventDefault();
    if (action === "submit") editor.submitInput();
    else if (action === "close") editor.closeInput();
    // finish() commits the open input first.
    else if (action === "copy" || action === "save") void editor.finish(action);
  };
}

function useAutoFocus(id: number) {
  const ref = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.focus({ preventScroll: true });
    el.setSelectionRange(el.value.length, el.value.length);
  }, [id]);
  return ref;
}

function MarkInput({ editor, input, view, t }: { editor: Editor; input: InputView; view: View; t: TFunction }) {
  const ref = useAutoFocus(input.id);
  // Tab between the input and its stop button keeps it open; focus leaving the box commits.
  const leave = (e: FocusEvent<HTMLElement>) => {
    if (!e.currentTarget.parentElement?.contains(e.relatedTarget)) editor.closeInput(input.id);
  };
  return (
    <Floating
      anchor={input.anchor}
      order={["right", "left", "below", "above"]}
      screen={view.screen}
      className={`mark-input glass${input.stoppable ? " stoppable" : ""}`}
    >
      <span className="mark-chip" style={{ background: input.color, color: contrastText(input.color) }} aria-hidden="true">
        {input.label}
      </span>
      <input
        ref={ref}
        value={input.value}
        maxLength={MAX_COMMENT}
        placeholder={t(input.caption ? "overlay.caption" : "overlay.comment")}
        aria-label={t(input.caption ? "overlay.captionLabel" : "overlay.commentLabel", { label: input.label })}
        spellCheck={false}
        autoComplete="off"
        onChange={(e) => editor.setInputValue(e.target.value)}
        onKeyDown={inputKeys(editor, view.mac)}
        onBlur={leave}
      />
      {input.stoppable && (
        <button
          type="button"
          className="caption-stop"
          title={t("overlay.captionsStopTip")}
          // Keeps the focus in the input, where the typing goes on.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => editor.stopCaptions()}
          onBlur={leave}
        >
          {t("overlay.captionsStop")}
        </button>
      )}
    </Floating>
  );
}

function TextInput({ editor, input, view, t }: { editor: Editor; input: InputView; view: View; t: TFunction }) {
  const ref = useAutoFocus(input.id);
  const a = input.anchor;
  return (
    <input
      ref={ref}
      className="text-input"
      value={input.value}
      maxLength={MAX_COMMENT}
      placeholder={t("overlay.textPlaceholder")}
      aria-label={t("overlay.textLabel")}
      spellCheck={false}
      autoComplete="off"
      style={{
        left: a.x,
        top: a.y,
        width: Math.max(a.w, input.fontPx * 3) + 2,
        height: a.h,
        padding: `${input.padY}px ${input.padX}px`,
        fontSize: input.fontPx,
        lineHeight: `${input.lineH}px`,
        color: input.plate ? contrastText(input.color) : input.color,
        background: input.plate ? input.color : "transparent",
        borderRadius: input.plate ? input.fontPx * 0.25 : 2,
      }}
      onChange={(e) => editor.setInputValue(e.target.value)}
      onKeyDown={inputKeys(editor, view.mac)}
      onBlur={() => editor.closeInput(input.id)}
    />
  );
}

const HELP_GROUPS: { key: string; ids: CommandId[] }[] = [
  {
    key: "tools",
    ids: ["tool.select", "tool.arrow", "tool.pencil", "tool.rect", "tool.marker", "tool.blur", "tool.mark", "tool.text"],
  },
  { key: "actions", ids: ["primary", "copy", "save", "addToSet", "commentPlacement", "captions", "escape"] },
  {
    key: "editing",
    ids: ["undo", "redo", "delete", "selectAll", "move", "moveFast", "resize", "resizeFast", "widthDown", "widthUp", "loupe", "pickColor", "help"],
  },
];

function commandLabel(id: CommandId, t: TFunction): string {
  return id.startsWith("tool.") ? t(`overlay.tools.${id.slice(5)}`) : t(`overlay.help.commands.${id}`);
}

function HelpPanel({ editor, view, t }: { editor: Editor; view: View; t: TFunction }) {
  return (
    <div className="help glass" role="dialog" aria-label={t("overlay.help.title")}>
      <div className="help-head">
        <h2>{t("overlay.help.title")}</h2>
        <button type="button" className="tb-btn" aria-label={t("overlay.help.close")} onMouseDown={(e) => e.preventDefault()} onClick={() => editor.toggleHelp()}>
          <CloseIcon />
        </button>
      </div>
      <div className="help-cols">
        {HELP_GROUPS.map((g) => (
          <section key={g.key}>
            <h3>{t(`overlay.help.groups.${g.key}`)}</h3>
            <dl>
              {g.ids.map((id) => {
                const binding = KEYMAP.find((b) => b.id === id);
                if (!binding) return null;
                return (
                  <div key={id} className="help-row">
                    <dt>{commandLabel(id, t)}</dt>
                    <dd>
                      {formatBinding(binding, view.mac).map((k) => (
                        <kbd key={k}>{k}</kbd>
                      ))}
                    </dd>
                  </div>
                );
              })}
            </dl>
          </section>
        ))}
      </div>
      <p className="help-foot">{t("overlay.help.footer")}</p>
    </div>
  );
}
