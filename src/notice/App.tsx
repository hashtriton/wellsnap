// Notice window: click-through toasts ("Copied"), the rare donation reminder card, the "updated" pill and the crash
// prompt.
import { useCallback, useEffect, useRef, useState } from "react";
import { AppMark } from "../assets/AppMark";
import { useI18n, type TFunction } from "../shared/i18n";
import { api, events, type CrashAnswer, type Lang, type NoticePayload, type PromptOutcome } from "../shared/ipc";
import { useThemeSync } from "../shared/theme";
import { hasPaymentMethods, RELEASES_URL, supportTarget } from "../shared/donate";
import { Button } from "../shared/ui/controls";
import { Icon } from "../shared/ui/icons";
import { crashCopy, nudgeCopy, toastDetail } from "./copy";
import {
  AutoHide,
  CRASH_AUTO_HIDE_MS,
  NUDGE_AUTO_HIDE_MS,
  outcomeOnHide,
  remainingAfterPause,
  UPDATED_AUTO_HIDE_MS,
} from "./nudge";

type Shown = { id: number; payload: NoticePayload };
type ClickOutcome = Exclude<PromptOutcome, "notSeen">;

export function App() {
  useThemeSync();
  const { lang, t } = useI18n();
  const [shown, setShown] = useState<Shown | null>(null);
  // The reminder currently on screen: when it appeared and whether its outcome was already reported.
  const card = useRef<{ shownAt: number; reported: boolean } | null>(null);

  const report = useCallback((outcome: PromptOutcome): boolean => {
    const current = card.current;
    if (!current || current.reported) return false;
    current.reported = true;
    api.donationOutcome(outcome).catch(() => undefined);
    return true;
  }, []);

  useEffect(() => {
    let seq = 0;
    // The card left the screen without a click: replaced by another notice or hidden by a new capture.
    const settleCard = () => {
      const current = card.current;
      if (current && !current.reported) report(outcomeOnHide(performance.now() - current.shownAt));
      card.current = null;
    };
    let alive = true;
    // Any event that arrived after the listeners were registered is newer than the core's pending notice.
    let heard = false;
    const show = (payload: NoticePayload) => {
      settleCard();
      if (payload.kind === "nudge" && !hasPaymentMethods()) {
        // This build has no way to donate, so the card would lead nowhere: retry later, spend nothing.
        api.donationOutcome("notSeen").catch(() => undefined);
        api.noticeClose().catch(() => undefined);
        setShown(null);
        return;
      }
      if (payload.kind === "nudge") card.current = { shownAt: performance.now(), reported: false };
      seq += 1;
      setShown({ id: seq, payload });
    };
    const subs = [
      events.onNotice((payload) => {
        heard = true;
        show(payload);
      }),
      events.onNoticeHide(() => {
        heard = true;
        settleCard();
        setShown(null);
      }),
    ];
    // The first notice of a run is emitted while this page is still loading; pull it once listening.
    Promise.all(subs)
      .then(() => api.noticeReady())
      .then((payload) => {
        if (alive && payload && !heard) show(payload);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      subs.forEach((p) => p.then((f) => f()));
    };
  }, [report]);

  const onOutcome = useCallback(
    (outcome: ClickOutcome) => {
      if (!report(outcome)) return;
      card.current = null;
      setShown(null);
      api.noticeClose().catch(() => undefined);
    },
    [report],
  );

  const closeNotice = useCallback(() => {
    setShown(null);
    api.noticeClose().catch(() => undefined);
  }, []);

  const payload = shown?.payload;
  return (
    <div className="notice-root" aria-live="polite">
      {payload?.kind === "toast" && (
        <Toast key={shown!.id} text={payload.text} detail={toastDetail(payload.detail)} error={payload.error} />
      )}
      {payload?.kind === "nudge" && (
        <NudgeCard key={shown!.id} index={payload.index} captures={payload.captures} lang={lang} t={t} onOutcome={onOutcome} />
      )}
      {payload?.kind === "updated" && (
        <UpdatedPill
          key={shown!.id}
          version={payload.version}
          hasNotes={payload.hasNotes}
          setCleared={payload.setCleared}
          t={t}
          onClose={closeNotice}
        />
      )}
      {payload?.kind === "crashPrompt" && <CrashCard key={shown!.id} t={t} onClose={closeNotice} />}
    </div>
  );
}

function Toast({ text, detail, error }: { text: string; detail: string | null; error: boolean }) {
  return (
    <div className={error ? "toast toast-error" : "toast"} role="status">
      <span className="toast-icon">
        <Icon name={error ? "alert" : "check"} size={15} />
      </span>
      <div className="toast-text">
        <div className={detail ? "toast-title" : "toast-title toast-title-wrap"}>{text}</div>
        {detail && <div className="toast-detail">{detail}</div>}
      </div>
    </div>
  );
}

/** After an automatic update: clickable, opens Settings > About; hides itself after 8 s unless hovered. */
function UpdatedPill({
  version,
  hasNotes,
  setCleared,
  t,
  onClose,
}: {
  version: string;
  hasNotes: boolean;
  setCleared: boolean;
  t: TFunction;
  onClose: () => void;
}) {
  const timer = useRef<AutoHide | null>(null);
  useEffect(() => {
    const autoHide = new AutoHide(UPDATED_AUTO_HIDE_MS, onClose);
    timer.current = autoHide;
    autoHide.start();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      autoHide.stop();
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);
  const hold = (key: "hovered" | "focused", on: boolean) => timer.current?.hold(key, on);

  const openAbout = () => {
    onClose();
    api.openSettings("about").catch(() => undefined);
  };
  // With notes, About shows them; without, the releases page lists the changes.
  const openNotes = () => {
    if (hasNotes) return openAbout();
    onClose();
    api
      .openExternal(RELEASES_URL)
      .catch(() => api.openSettings("about"))
      .catch(() => undefined);
  };

  // The whole pill opens About for the mouse; the title is the same action as a real button. The live region
  // is .notice-root, so the pill itself has no role.
  return (
    <div
      className="toast updated"
      onClick={openAbout}
      onMouseEnter={() => hold("hovered", true)}
      onMouseLeave={() => hold("hovered", false)}
      onFocus={() => hold("focused", true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) hold("focused", false);
      }}
    >
      <span className="toast-icon updated-icon">
        <Icon name="refresh" size={15} />
      </span>
      <div className="toast-text">
        <button
          type="button"
          className="toast-title updated-title"
          onClick={(e) => {
            e.stopPropagation();
            openAbout();
          }}
        >
          {t("notice.updated.title", { version })}
        </button>
        {setCleared && <div className="toast-detail">{t("notice.updated.setCleared")}</div>}
        <button
          type="button"
          className="link-btn updated-link"
          onClick={(e) => {
            e.stopPropagation();
            openNotes();
          }}
        >
          {t(hasNotes ? "notice.updated.whatsNew" : "notice.updated.changelog")}
        </button>
      </div>
      <button
        type="button"
        className="updated-close"
        aria-label={t("notice.updated.close")}
        title={t("notice.updated.close")}
        onClick={(e) => {
          e.stopPropagation();
          onClose();
        }}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
  );
}

function NudgeCard({
  index,
  captures,
  lang,
  t,
  onOutcome,
}: {
  index: number;
  captures: number;
  lang: Lang;
  t: TFunction;
  onOutcome: (outcome: ClickOutcome) => void;
}) {
  const { title, body } = nudgeCopy(t, index, captures);

  // Auto-hide after 12 s, paused while the pointer or keyboard focus is on the card.
  const timer = useRef<{ id?: number; startedAt: number; budget: number; hovered: boolean; focused: boolean }>({
    startedAt: 0,
    budget: NUDGE_AUTO_HIDE_MS,
    hovered: false,
    focused: false,
  });
  const start = useCallback(() => {
    const s = timer.current;
    window.clearTimeout(s.id);
    s.startedAt = performance.now();
    s.id = window.setTimeout(() => onOutcome("later"), s.budget);
  }, [onOutcome]);
  const pause = useCallback(() => {
    const s = timer.current;
    if (s.id === undefined) return;
    window.clearTimeout(s.id);
    s.id = undefined;
    s.budget = remainingAfterPause(s.budget, performance.now() - s.startedAt);
  }, []);
  const setHold = useCallback(
    (key: "hovered" | "focused", value: boolean) => {
      const s = timer.current;
      s[key] = value;
      if (s.hovered || s.focused) pause();
      else if (s.id === undefined) start();
    },
    [pause, start],
  );

  useEffect(() => {
    start();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onOutcome("later");
    };
    window.addEventListener("keydown", onKey);
    const s = timer.current;
    return () => {
      window.clearTimeout(s.id);
      window.removeEventListener("keydown", onKey);
    };
  }, [start, onOutcome]);

  const support = () => {
    onOutcome("support");
    const target = supportTarget(lang);
    const opened = target.kind === "url" ? api.openExternal(target.url) : Promise.reject(new Error("no link"));
    opened.catch(() => api.openSettings("support")).catch(() => undefined);
  };

  return (
    <section
      className="nudge"
      aria-labelledby="nudge-title"
      aria-describedby="nudge-body"
      onMouseEnter={() => setHold("hovered", true)}
      onMouseLeave={() => setHold("hovered", false)}
      onFocus={() => setHold("focused", true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setHold("focused", false);
      }}
    >
      <header className="nudge-head">
        <AppMark size={22} />
        <h1 id="nudge-title">{title}</h1>
      </header>
      <p id="nudge-body" className="nudge-body">
        {body}
      </p>
      <div className="nudge-actions">
        <div className="nudge-buttons">
          <Button variant="primary" icon="heart" onClick={support}>
            {t("notice.support")}
          </Button>
          <Button onClick={() => onOutcome("later")}>{t("notice.later")}</Button>
        </div>
        <div className="nudge-links">
          <button type="button" className="link-btn" onClick={() => onOutcome("alreadySupported")}>
            {t("notice.alreadySupported")}
          </button>
          <button type="button" className="link-btn" onClick={() => onOutcome("never")}>
            {t("notice.never")}
          </button>
        </div>
      </div>
    </section>
  );
}

/**
 * After a crash in "ask" mode. Hides itself after 20 s unless hovered or focused; Esc, "Details" and the timeout
 * leave the reports waiting without an answer.
 */
function CrashCard({ t, onClose }: { t: TFunction; onClose: () => void }) {
  const copy = crashCopy(t);
  const timer = useRef<AutoHide | null>(null);
  useEffect(() => {
    const autoHide = new AutoHide(CRASH_AUTO_HIDE_MS, onClose);
    timer.current = autoHide;
    autoHide.start();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      autoHide.stop();
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);
  const hold = (key: "hovered" | "focused", on: boolean) => timer.current?.hold(key, on);

  const answer = (choice: Exclude<CrashAnswer, "delete">) => {
    api.crashPromptAnswer(choice).catch(() => undefined);
    onClose();
  };
  const details = () => {
    onClose();
    api.openSettings("privacy").catch(() => undefined);
  };

  return (
    <section
      className="nudge"
      aria-labelledby="crash-title"
      aria-describedby="crash-body"
      onMouseEnter={() => hold("hovered", true)}
      onMouseLeave={() => hold("hovered", false)}
      onFocus={() => hold("focused", true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) hold("focused", false);
      }}
    >
      <header className="nudge-head">
        <AppMark size={22} />
        <h1 id="crash-title">{copy.title}</h1>
      </header>
      <p id="crash-body" className="nudge-body">
        {copy.body}
      </p>
      <div className="crash-actions">
        <div className="nudge-buttons">
          <Button variant="primary" onClick={() => answer("send")}>
            {copy.send}
          </Button>
          <Button onClick={() => answer("dontSend")}>{copy.dontSend}</Button>
        </div>
        <div className="crash-links">
          <button type="button" className="link-btn" onClick={() => answer("always")}>
            {copy.always}
          </button>
          <button type="button" className="link-btn" onClick={details}>
            {copy.details}
          </button>
        </div>
      </div>
    </section>
  );
}
