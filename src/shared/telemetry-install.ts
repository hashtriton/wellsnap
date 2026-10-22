// The first import of every page's main.tsx. Imports are evaluated before the importing module's body, so a call
// in main.tsx itself would miss errors thrown while the page's other modules are being evaluated.
import { installErrorReporting } from "./telemetry";

installErrorReporting();
