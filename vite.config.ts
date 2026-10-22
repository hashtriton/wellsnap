/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

// JS crash reports (src/shared/telemetry.ts) point into minified chunks. Only a release build given SENTRY_AUTH_TOKEN
// injects debug ids, uploads hidden source maps to Sentry and deletes them before Tauri embeds dist; every other
// build stays as before and does not even load the plugin.
const sentryToken = process.env.SENTRY_AUTH_TOKEN?.trim();
const sentry = sentryToken
  ? (await import("@sentry/vite-plugin")).sentryVitePlugin({
      org: process.env.SENTRY_ORG || "wellsnap-zw",
      project: process.env.SENTRY_PROJECT || "wellsnap",
      authToken: sentryToken,
      telemetry: false,
      // Releases (wellsnap@<version>) belong to the Rust side; debug ids do not need one.
      release: { inject: false, create: false, finalize: false, setCommits: false },
      sourcemaps: { filesToDeleteAfterUpload: ["./dist/**/*.map"] },
    })
  : [];

// Each Tauri window loads its own page; there is no "main" window (tray app).
const pages = ["overlay", "strip", "notice", "settings", "onboarding"];
const root = fileURLToPath(new URL(".", import.meta.url));

export default defineConfig({
  plugins: [react(), ...sentry],
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
    sourcemap: sentryToken ? "hidden" : false,
    rollupOptions: {
      input: Object.fromEntries(pages.map((p) => [p, `${root}${p}.html`])),
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
