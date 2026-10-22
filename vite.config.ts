/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

// Each Tauri window loads its own page; there is no "main" window (tray app).
const pages = ["overlay", "strip", "notice", "settings", "onboarding"];
const root = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // IPv4 explicitly: Node resolves "localhost" to ::1 and the Tauri CLI then never sees the server.
    host: host || "127.0.0.1",
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // WebView2 (Chromium) on Windows, WKWebView of macOS 14+ (Safari 17).
    target: ["es2022", "chrome120", "safari17"],
    rollupOptions: {
      input: Object.fromEntries(pages.map((p) => [p, `${root}${p}.html`])),
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
