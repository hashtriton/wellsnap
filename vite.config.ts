/// <reference types="vitest/config" />
import { defineConfig, type ConfigEnv, type PluginOption, type UserConfig } from "vite";
import react from "@vitejs/plugin-react";
import { Buffer } from "node:buffer";
import { fileURLToPath } from "node:url";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

// Each Tauri window loads its own page; there is no "main" window (tray app).
const pages = ["overlay", "strip", "notice", "settings", "onboarding"];
const root = fileURLToPath(new URL(".", import.meta.url));

const sentryOrg = process.env.SENTRY_ORG || "wellsnap-zw";

/** Org auth tokens are sntrys_<base64 JSON>_<secret> and the JSON names the org; user tokens say nothing. */
function tokenOrg(token: string): string | undefined {
  const parts = token.split("_");
  if (parts[0] !== "sntrys" || parts.length !== 3) return undefined;
  try {
    const org: unknown = JSON.parse(Buffer.from(parts[1], "base64").toString("utf8"))?.org;
    return typeof org === "string" && org ? org : undefined;
  } catch {
    return undefined;
  }
}

// JS crash reports (src/shared/telemetry.ts) point into minified chunks. Only `vite build` with a SENTRY_AUTH_TOKEN
// that may write to our org injects debug ids, uploads hidden source maps to Sentry and deletes them before Tauri
// embeds dist. Dev, vitest and every other build do not load the plugin and build no maps.
async function sentryUpload(command: ConfigEnv["command"]): Promise<PluginOption[]> {
  const authToken = process.env.SENTRY_AUTH_TOKEN?.trim();
  if (command !== "build" || !authToken) return [];
  const org = tokenOrg(authToken);
  if (org && org !== sentryOrg) {
    console.warn(`[sentry] SENTRY_AUTH_TOKEN is for the org "${org}", not "${sentryOrg}": source maps are not uploaded.`);
    return [];
  }
  // The Sentry CLI inside the plugin reports its own errors to Sentry unless told not to.
  process.env.SENTRY_CLI_NO_TELEMETRY ??= "1";
  const { sentryVitePlugin } = await import("@sentry/vite-plugin");
  return sentryVitePlugin({
    org: sentryOrg,
    project: process.env.SENTRY_PROJECT || "wellsnap",
    authToken,
    telemetry: false,
    // Releases (wellsnap@<version>) belong to the Rust side; debug ids do not need one.
    release: { inject: false, create: false, finalize: false, setCommits: false },
    // Absolute, so the maps go whatever the working directory; glob patterns want forward slashes.
    sourcemaps: { filesToDeleteAfterUpload: [`${root.replaceAll("\\", "/")}dist/**/*.map`] },
  });
}

export default defineConfig(async ({ command }): Promise<UserConfig> => {
  const sentry = await sentryUpload(command);
  return {
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
      sourcemap: sentry.length > 0 ? "hidden" : false,
      rollupOptions: {
        input: Object.fromEntries(pages.map((p) => [p, `${root}${p}.html`])),
      },
    },
    test: {
      environment: "node",
      include: ["src/**/*.test.ts"],
    },
  };
});
