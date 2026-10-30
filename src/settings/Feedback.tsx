// "Feedback" section: a message to the author, sent by the core to wellsnap.net, which mails it on (feedback.rs).
import { useId, useState } from "react";
import { api, type FeedbackError } from "../shared/ipc";
import { copyText } from "../shared/ui/clipboard";
import { Button, Notice } from "../shared/ui/controls";
import { PageHead } from "./General";
import type { PageProps } from "./types";

export const FEEDBACK_EMAIL = "wellsnap@welldone.one";
/** Same limit as the core and the endpoint. */
export const MESSAGE_MAX = 5000;

type SendState = "idle" | "sending" | "sent" | FeedbackError;

/** Survives a switch to another tab: the page unmounts, a half-written message should not go with it. */
const draft = { message: "", email: "" };

/** Failures the user can fix in the form; the rest offer the address to write to instead. */
const FIXABLE: FeedbackError[] = ["empty", "tooLong", "badEmail"];

export function Feedback({ t, info }: PageProps) {
  const id = useId();
  const [message, setMessage] = useState(draft.message);
  const [email, setEmail] = useState(draft.email);
  const [state, setState] = useState<SendState>("idle");
  const [copied, setCopied] = useState(false);
  const length = [...message.trim()].length;
  const tooLong = length > MESSAGE_MAX;

  const edit = (next: { message?: string; email?: string }) => {
    Object.assign(draft, next);
    if (next.message !== undefined) setMessage(next.message);
    if (next.email !== undefined) setEmail(next.email);
    if (state !== "sending") setState("idle");
  };

  const send = async () => {
    setState("sending");
    setCopied(false);
    try {
      await api.sendFeedback(message, email);
      // The address stays for the next message.
      draft.message = "";
      setMessage("");
      setState("sent");
    } catch (e) {
      setState(isFeedbackError(e) ? e : "failed");
    }
  };

  const copyAddress = async () => setCopied(await copyText(FEEDBACK_EMAIL));
  const error = isFeedbackError(state) ? state : null;
  const fixable = error !== null && FIXABLE.includes(error);

  return (
    <>
      <PageHead title={t("settings.feedback.title")} />
      <p className="lead">{t("settings.feedback.lead")}</p>
      <section className="group feedback-form">
        <label className="feedback-label" htmlFor={`${id}-message`}>
          {t("settings.feedback.message")}
        </label>
        <textarea
          id={`${id}-message`}
          className="feedback-input feedback-message"
          rows={8}
          value={message}
          disabled={state === "sending"}
          placeholder={t("settings.feedback.messagePlaceholder")}
          aria-invalid={tooLong || state === "empty" || state === "tooLong" || undefined}
          aria-describedby={`${id}-count`}
          onChange={(e) => edit({ message: e.target.value })}
        />
        <div id={`${id}-count`} className={`feedback-count${tooLong ? " feedback-count-over" : ""}`}>
          {t("settings.feedback.count", { count: length, max: MESSAGE_MAX })}
        </div>
        <label className="feedback-label" htmlFor={`${id}-email`}>
          {t("settings.feedback.email")}
        </label>
        <input
          id={`${id}-email`}
          className="feedback-input"
          type="email"
          autoComplete="email"
          spellCheck={false}
          value={email}
          disabled={state === "sending"}
          placeholder={t("settings.feedback.emailPlaceholder")}
          aria-invalid={state === "badEmail" || undefined}
          aria-describedby={`${id}-email-desc`}
          onChange={(e) => edit({ email: e.target.value })}
        />
        <div id={`${id}-email-desc`} className="feedback-hint">
          {t("settings.feedback.emailDesc")}
        </div>
        <p className="feedback-hint">
          {t("settings.feedback.attached", { version: info?.version ?? "?", os: info?.os ?? "?", lang: info?.lang ?? "?" })}
        </p>
        <div className="card-actions">
          <Button
            variant="primary"
            icon="message"
            busy={state === "sending"}
            disabled={length === 0 || tooLong}
            onClick={send}
          >
            {t("settings.feedback.send")}
          </Button>
        </div>
      </section>
      {state === "sent" && <Notice tone="success">{t("settings.feedback.sent")}</Notice>}
      {error && (
        <Notice tone={fixable ? "warning" : "error"}>
          {t(`settings.feedback.error.${error}`)}
          {!fixable && (
            <span className="feedback-fallback">
              {t("settings.feedback.writeDirect")} <span className="feedback-address">{FEEDBACK_EMAIL}</span>{" "}
              <button type="button" className="link-btn" onClick={copyAddress}>
                {copied ? t("settings.feedback.copied") : t("settings.feedback.copy")}
              </button>
            </span>
          )}
        </Notice>
      )}
      <p className="footnote">{t("settings.feedback.privacy")}</p>
    </>
  );
}

/** The core's FeedbackError values (feedback.rs). */
export const FEEDBACK_ERRORS: readonly FeedbackError[] = ["empty", "tooLong", "badEmail", "rateLimited", "dailyLimit", "offline", "failed"];

export function isFeedbackError(value: unknown): value is FeedbackError {
  return typeof value === "string" && (FEEDBACK_ERRORS as readonly string[]).includes(value);
}
