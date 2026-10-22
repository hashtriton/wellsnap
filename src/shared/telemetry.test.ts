import { afterEach, describe as group, expect, it, vi } from "vitest";
import { api, type JsErrorReport } from "./ipc";
import {
  attach,
  createReporter,
  debugIdsByUrl,
  describe,
  installErrorReporting,
  MAX_MESSAGE,
  MAX_REPORTS,
  MAX_STACK,
  REPORT_WINDOW_MS,
  truncate,
} from "./telemetry";

const setup = (limits?: Parameters<typeof createReporter>[2]) => {
  const sent: JsErrorReport[] = [];
  const send = vi.fn((r: JsErrorReport) => {
    sent.push(r);
    return Promise.resolve();
  });
  const report = createReporter(send, () => ({ "app:///assets/a.js": "d1" }), limits);
  return { sent, send, report };
};

group("truncate", () => {
  it("keeps short text and cuts long text to the limit", () => {
    expect(truncate("abc", 3)).toBe("abc");
    expect(truncate("abcdef", 4)).toBe("abc…");
    expect(truncate("x".repeat(600), MAX_MESSAGE)).toHaveLength(MAX_MESSAGE);
  });

  it("never splits a surrogate pair", () => {
    const cut = truncate(`ab${"😀".repeat(5)}`, 4);
    expect(cut).toBe("ab…");
    expect(cut.length).toBeLessThanOrEqual(4);
  });
});

group("describe", () => {
  it("reads errors, strings and other values", () => {
    const err = new TypeError("x is not a function");
    expect(describe(err)).toEqual({ message: "TypeError: x is not a function", stack: err.stack });
    expect(describe("could not save")).toEqual({ message: "could not save", stack: null });
    expect(describe(42)).toEqual({ message: "42", stack: null });
    expect(describe(false)).toEqual({ message: "false", stack: null });
    expect(describe(undefined)).toEqual({ message: "undefined", stack: null });
    expect(describe(null, "Uncaught boom")).toEqual({ message: "Uncaught boom", stack: null });
  });

  it("gives only the shape of other objects, never their values", () => {
    expect(describe({ code: 5, path: "C:\Users\anna\shot.png", comment: "my note" })).toEqual({
      message: "Object{code,path,comment}",
      stack: null,
    });
    expect(describe(["C:\Users\anna"]).message).toBe("Array(1)");
    expect(describe(new Map([["k", "v"]])).message).toBe("Map{}");
    expect(describe(Object.create(null)).message).toBe("Object{}");
    expect(describe(Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`k${i}`, i]))).message).toBe(
      "Object{k0,k1,k2,k3,k4,k5,k6,k7,…}",
    );
    expect(describe(() => "secret").message).toBe("function");
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    expect(describe(cyclic).message).toBe("Object{self}");
    expect(describe(10n).message).toBe("10");
  });

  it("accepts error-like objects from another realm", () => {
    expect(describe({ name: "DOMException", message: "aborted" })).toEqual({ message: "DOMException: aborted", stack: null });
  });
});

group("createReporter", () => {
  it("sends the kind, message, stack and debug ids", () => {
    const { sent, report } = setup();
    const err = new Error("boom");
    expect(report("error", err)).toBe(true);
    expect(sent).toEqual([{ kind: "error", message: "Error: boom", stack: err.stack, debugIds: { "app:///assets/a.js": "d1" } }]);
  });

  it("sends the same message once per page load", () => {
    const { send, report } = setup();
    expect(report("error", new Error("boom"))).toBe(true);
    expect(report("error", new Error("boom"))).toBe(false);
    expect(report("unhandledRejection", new Error("boom"))).toBe(true);
    expect(send).toHaveBeenCalledTimes(2);
  });

  it("stops after the per-page limit", () => {
    const { send, report } = setup();
    for (let i = 0; i < MAX_REPORTS + 3; i++) report("error", new Error(`boom ${i}`));
    expect(send).toHaveBeenCalledTimes(MAX_REPORTS);
  });

  it("starts counting and deduping again in the next window", () => {
    let time = 1_000;
    const { send, report } = setup({ now: () => time });
    for (let i = 0; i < MAX_REPORTS; i++) report("error", new Error(`boom ${i}`));
    expect(report("error", new Error("new"))).toBe(false);
    time += REPORT_WINDOW_MS - 1;
    expect(report("error", new Error("new"))).toBe(false);
    time += 1;
    expect(report("error", new Error("boom 0"))).toBe(true);
    expect(report("error", new Error("boom 0"))).toBe(false);
    expect(report("error", new Error("new"))).toBe(true);
    expect(send).toHaveBeenCalledTimes(MAX_REPORTS + 2);
  });

  it("truncates long messages and stacks", () => {
    const { sent, report } = setup();
    report("unhandledRejection", { name: "Error", message: "m".repeat(2000), stack: "s".repeat(20_000) });
    expect(sent[0].message).toHaveLength(MAX_MESSAGE);
    expect(sent[0].stack).toHaveLength(MAX_STACK);
    // Messages that differ only after the cut count as one.
    report("unhandledRejection", { name: "Error", message: `${"m".repeat(2000)}tail` });
    expect(sent).toHaveLength(1);
  });

  it("never throws, even when sending or reading debug ids fails", async () => {
    const failing = createReporter(() => Promise.reject(new Error("no command")), () => ({}));
    expect(failing("error", new Error("a"))).toBe(true);
    const broken = createReporter(
      () => Promise.resolve(),
      () => {
        throw new Error("bad map");
      },
    );
    expect(broken("error", new Error("b"))).toBe(false);
    await new Promise((r) => setTimeout(r, 0));
  });
});

group("debugIdsByUrl", () => {
  it("maps the first script URL of each snippet stack to its debug id", () => {
    const raw = {
      "Error\n    at http://tauri.localhost/assets/settings-B1x.js:1:2345": "id-v8",
      "@tauri://localhost/assets/notice-C2y.js:1:99\nglobal code@tauri://localhost/assets/other.js:3:1": "id-jsc",
      "Error\n    at Object.<anonymous> (http://localhost:1420/src/main.tsx:3:5)": "id-dev",
      "Error\n    at nowhere": "id-none",
      "Error\n    at http://tauri.localhost/assets/x.js:1:1": 7,
    };
    expect(debugIdsByUrl(raw)).toEqual({
      "http://tauri.localhost/assets/settings-B1x.js": "id-v8",
      "tauri://localhost/assets/notice-C2y.js": "id-jsc",
      "http://localhost:1420/src/main.tsx": "id-dev",
    });
  });

  it("reads what the plugin's injected snippet writes", () => {
    // getDebugIdSnippet of @sentry/bundler-plugins 11.8.0 inside its CodeInjection wrapper, as it opens each chunk.
    const id = "4f2c8a1e-0b7d-4e3a-9c55-1d2e3f4a5b6c";
    const snippet = `!function(){try{var e="undefined"!=typeof window?window:"undefined"!=typeof global?global:"undefined"!=typeof globalThis?globalThis:"undefined"!=typeof self?self:{};var n=(new e.Error).stack;n&&(e._sentryDebugIds=e._sentryDebugIds||{},e._sentryDebugIds[n]="${id}",e._sentryDebugIdIdentifier="sentry-dbid-${id}");}catch(e){}}();`;
    // Run as the chunks would be: sourceURL gives the frames their script URL, as the webview does.
    const page = globalThis as { _sentryDebugIds?: unknown; _sentryDebugIdIdentifier?: unknown };
    const run = (code: string, url: string) => (0, eval)(`${code}
//# sourceURL=${url}`);
    try {
      run(snippet, "http://tauri.localhost/assets/settings-B1x.js");
      run(snippet.replaceAll(id, "other-id"), "http://tauri.localhost/assets/ipc-C9z.js");
      expect(debugIdsByUrl(page._sentryDebugIds)).toEqual({
        "http://tauri.localhost/assets/settings-B1x.js": id,
        "http://tauri.localhost/assets/ipc-C9z.js": "other-id",
      });
    } finally {
      delete page._sentryDebugIds;
      delete page._sentryDebugIdIdentifier;
    }
  });

  it("is empty without the plugin", () => {
    expect(debugIdsByUrl(undefined)).toEqual({});
    expect(debugIdsByUrl("x")).toEqual({});
  });
});

group("attach", () => {
  it("reports window errors and unhandled rejections", () => {
    const target = new EventTarget();
    const report = vi.fn(() => true);
    attach(target, report);
    const err = new Error("render failed");
    target.dispatchEvent(Object.assign(new Event("error"), { error: err, message: "Uncaught Error: render failed" }));
    target.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: "command failed" }));
    target.dispatchEvent(Object.assign(new Event("error"), { error: null, message: "Script error." }));
    expect(report.mock.calls).toEqual([
      ["error", err, "Uncaught Error: render failed"],
      ["unhandledRejection", "command failed"],
      ["error", null, "Script error."],
    ]);
  });
});

group("installErrorReporting", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("listens once, however often it is called", () => {
    const page = new EventTarget();
    const listen = vi.spyOn(page, "addEventListener");
    const send = vi.spyOn(api, "reportJsError").mockResolvedValue(undefined);
    vi.stubGlobal("window", page);
    installErrorReporting();
    installErrorReporting();
    expect(listen.mock.calls.map(([type]) => type)).toEqual(["error", "unhandledrejection"]);
    page.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: "command failed" }));
    expect(send).toHaveBeenCalledWith({ kind: "unhandledRejection", message: "command failed", stack: null, debugIds: {} });
  });
});
