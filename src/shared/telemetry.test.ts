import { describe as group, expect, it, vi } from "vitest";
import type { JsErrorReport } from "./ipc";
import { attach, createReporter, debugIdsByUrl, describe, MAX_MESSAGE, MAX_REPORTS, MAX_STACK, truncate } from "./telemetry";

const setup = (max?: number) => {
  const sent: JsErrorReport[] = [];
  const send = vi.fn((r: JsErrorReport) => {
    sent.push(r);
    return Promise.resolve();
  });
  const report = createReporter(send, () => ({ "app:///assets/a.js": "d1" }), max);
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
    expect(describe({ code: 5 })).toEqual({ message: '{"code":5}', stack: null });
    expect(describe(42)).toEqual({ message: "42", stack: null });
    expect(describe(undefined)).toEqual({ message: "undefined", stack: null });
    expect(describe(null, "Uncaught boom")).toEqual({ message: "Uncaught boom", stack: null });
  });

  it("survives values JSON cannot write", () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    expect(describe(cyclic).message).toBe("[object Object]");
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
