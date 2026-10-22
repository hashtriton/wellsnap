// JavaScript errors of a webview go to the core (report_js_error), which knows the page from the window label and
// decides whether anything is sent (a build with telemetry, the crash report setting, onboarding done). React 19
// reports uncaught render errors through window.reportError, so they arrive here as window "error" events too.
import { api, type JsErrorKind, type JsErrorReport } from "./ipc";

/** Reports per page load; a page stuck in an error loop must not flood the outbox. */
export const MAX_REPORTS = 5;
export const MAX_MESSAGE = 500;
export const MAX_STACK = 8192;

export type ReportFn = (kind: JsErrorKind, value: unknown, fallbackMessage?: string) => boolean;

/** Cuts to `max` characters with an ellipsis, never splitting a surrogate pair. */
export function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  let cut = text.slice(0, max - 1);
  if (/[\uD800-\uDBFF]$/.test(cut)) cut = cut.slice(0, -1);
  return `${cut}…`;
}

/**
 * Message and stack of anything thrown or rejected: an Error from any realm, a string (Tauri command errors) or any
 * other value.
 */
export function describe(value: unknown, fallbackMessage = ""): { message: string; stack: string | null } {
  if (value && typeof value === "object" && typeof (value as Error).message === "string") {
    const { name, message, stack } = value as Error;
    return { message: name ? `${name}: ${message}` : message, stack: typeof stack === "string" && stack ? stack : null };
  }
  if (typeof value === "string") return { message: value, stack: null };
  if (value === undefined || value === null) return { message: fallbackMessage || String(value), stack: null };
  let text: string | undefined;
  try {
    text = JSON.stringify(value);
  } catch {
    // Cycles and BigInt: the plain conversion below still says what it was.
  }
  return { message: text ?? String(value), stack: null };
}

const FRAME_URL = /([a-z][a-z0-9+.-]*:\/\/[^\s()@]+?)(?::\d+){1,2}\)?\s*$/i;

/**
 * `globalThis._sentryDebugIds` (injected by @sentry/vite-plugin) maps the stack of each chunk's snippet to the chunk's
 * debug id; the core wants script URL to debug id. Reads V8 (`at url:1:2`) and JSC (`@url:1:2`) frames.
 */
export function debugIdsByUrl(raw: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  if (!raw || typeof raw !== "object") return out;
  for (const [stack, id] of Object.entries(raw as Record<string, unknown>)) {
    if (typeof id !== "string") continue;
    for (const line of stack.split("\n")) {
      const url = FRAME_URL.exec(line)?.[1];
      if (url) {
        out[url] = id;
        break;
      }
    }
  }
  return out;
}

/** Dedupes by kind and message and stops after `max` reports; never throws, whatever `send` does. */
export function createReporter(
  send: (report: JsErrorReport) => Promise<unknown>,
  readDebugIds: () => Record<string, string>,
  max = MAX_REPORTS,
): ReportFn {
  const seen = new Set<string>();
  return (kind, value, fallbackMessage) => {
    try {
      if (seen.size >= max) return false;
      const { message, stack } = describe(value, fallbackMessage);
      const short = truncate(message, MAX_MESSAGE);
      const key = `${kind}\n${short}`;
      if (seen.has(key)) return false;
      seen.add(key);
      const report: JsErrorReport = {
        kind,
        message: short,
        stack: stack === null ? null : truncate(stack, MAX_STACK),
        debugIds: readDebugIds(),
      };
      send(report).catch(() => undefined);
      return true;
    } catch {
      return false;
    }
  };
}

/** Wires a reporter to a window's (or any event target's) "error" and "unhandledrejection" events. */
export function attach(target: Pick<EventTarget, "addEventListener">, report: ReportFn) {
  target.addEventListener("error", (e) => {
    const event = e as ErrorEvent;
    report("error", event.error, event.message);
  });
  target.addEventListener("unhandledrejection", (e) => report("unhandledRejection", (e as PromiseRejectionEvent).reason));
}

let installed = false;

/** First thing in every page's main.tsx. */
export function installErrorReporting() {
  if (installed) return;
  installed = true;
  attach(
    window,
    createReporter(api.reportJsError, () => debugIdsByUrl((globalThis as { _sentryDebugIds?: unknown })._sentryDebugIds)),
  );
}
