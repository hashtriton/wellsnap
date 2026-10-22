// Records a global hotkey. While it has focus every global hotkey is unregistered (hotkeys_suspend),
// otherwise the OS would deliver the combination to WellSnap instead of to this field. The core also
// re-registers them when the Settings window is destroyed, since nothing here is sure to run on close.
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { api, type Platform } from "../shared/ipc";
import type { TFunction } from "../shared/i18n";
import { Keys } from "../shared/ui/controls";
import { formatAccelerator, modifierLabel, recordKey, type InvalidReason } from "../shared/ui/hotkey";
import { Icon } from "../shared/ui/icons";

export function HotkeyRecorder({
  value,
  action,
  platform,
  t,
  describedBy,
  busy,
  onRecord,
  onInvalid,
  onCancel,
}: {
  value: string;
  action: string;
  platform: Platform;
  t: TFunction;
  describedBy?: string;
  busy?: boolean;
  /** Returns true when the combination was saved; false keeps recording (the caller shows why). */
  onRecord: (accelerator: string) => Promise<boolean>;
  onInvalid: (reason: InvalidReason) => void;
  /** Recording stopped without saving (Esc or focus left). */
  onCancel: () => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  const suspended = useRef(false);
  const downCodes = useRef(new Set<string>());
  const [recording, setRecording] = useState(false);
  const [held, setHeld] = useState<string[]>([]);

  const suspend = () => {
    if (suspended.current) return;
    suspended.current = true;
    api.hotkeysSuspend().catch(() => undefined);
  };
  const resume = () => {
    if (!suspended.current) return;
    suspended.current = false;
    api.hotkeysResume().catch(() => undefined);
  };

  useEffect(() => {
    // Best effort when the window closes: React cleanups do not run on page unload.
    window.addEventListener("pagehide", resume);
    return () => {
      window.removeEventListener("pagehide", resume);
      resume();
    };
  }, []);

  const start = () => {
    setRecording(true);
    setHeld([]);
    downCodes.current.clear();
    suspend();
  };
  const stop = () => {
    setRecording(false);
    setHeld([]);
    resume();
  };

  const submit = async (accelerator: string) => {
    setHeld([]);
    if (await onRecord(accelerator)) {
      stop();
      ref.current?.blur();
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        start();
      }
      return;
    }
    // Plain Tab keeps working for keyboard navigation.
    if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat || busy) return;
    downCodes.current.add(e.code);
    const result = recordKey(e, platform);
    switch (result.kind) {
      case "pending":
        setHeld(result.modifiers);
        break;
      case "cancel":
        onCancel();
        stop();
        ref.current?.blur();
        break;
      case "ok":
        void submit(result.accelerator);
        break;
      case "invalid":
        setHeld([]);
        onInvalid(result.reason);
        break;
    }
  };

  const onKeyUp = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    e.preventDefault();
    // WebView2 usually reports Print Screen only on key up.
    if (e.code === "PrintScreen" && !downCodes.current.has("PrintScreen") && !busy) {
      const result = recordKey(e, platform);
      if (result.kind === "ok") void submit(result.accelerator);
      return;
    }
    downCodes.current.delete(e.code);
    const result = recordKey({ ...pickModifiers(e), code: "" }, platform);
    if (result.kind === "pending") setHeld(result.modifiers);
  };

  const shown = formatAccelerator(value, platform) || t("settings.hotkeys.notSet");
  return (
    <button
      ref={ref}
      type="button"
      className={`recorder${recording ? " recorder-on" : ""}`}
      aria-describedby={describedBy}
      aria-label={t("settings.hotkeys.recorderLabel", { action, hotkey: shown })}
      aria-busy={busy || undefined}
      onFocus={start}
      onBlur={() => {
        if (recording) onCancel();
        stop();
      }}
      onClick={() => !recording && start()}
      onKeyDown={onKeyDown}
      onKeyUp={onKeyUp}
    >
      {recording ? (
        <span className="recorder-live">
          {held.length > 0 ? (
            <span className="keys">
              {held.map((m) => (
                <kbd key={m} className="kbd">
                  {modifierLabel(m, platform)}
                </kbd>
              ))}
              <span className="recorder-plus">+</span>
            </span>
          ) : (
            <span className="recorder-dot" aria-hidden="true" />
          )}
          <span>{t("settings.hotkeys.recording")}</span>
        </span>
      ) : value ? (
        <Keys accelerator={value} platform={platform} />
      ) : (
        <span className="recorder-empty">{shown}</span>
      )}
      <Icon name="keyboard" size={16} className="recorder-icon" />
    </button>
  );
}

function pickModifiers(e: KeyboardEvent) {
  return { ctrlKey: e.ctrlKey, altKey: e.altKey, shiftKey: e.shiftKey, metaKey: e.metaKey };
}
