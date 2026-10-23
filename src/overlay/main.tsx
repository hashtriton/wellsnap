// Capture overlay: one hidden window per monitor. The frame path (frame -> canvas -> overlayReady) stays
// outside React so it is never delayed by rendering; React only draws the UI layer on top of the canvas.
import "../shared/telemetry-install";
import "../shared/theme.css";
import "./overlay.css";
import { createRoot } from "react-dom/client";
import { api, events } from "../shared/ipc";
import { Editor } from "./editor";
import { listenForFrames } from "./frame";
import { watchLiveRegion } from "./live";
import { App } from "./ui/App";

const root = document.getElementById("root")!;
const canvas = document.createElement("canvas");
canvas.className = "stage";
document.body.insertBefore(canvas, root);

const editor = new Editor(canvas);
listenForFrames((payload, rgba) => editor.onFrame(payload, rgba));
events.onOverlayReset(() => editor.reset());
events.onSettingsChanged((s) => editor.applySettings(s));
api
  .getSettings()
  .then((s) => editor.applySettings(s))
  .catch(() => {});

createRoot(root).render(<App editor={editor} />);
watchLiveRegion(editor, root);
