import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../shared/theme.css";
import "../shared/ui/ui.css";
import "./onboarding.css";
import { installErrorReporting } from "../shared/telemetry";
import { App } from "./App";

installErrorReporting();
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
