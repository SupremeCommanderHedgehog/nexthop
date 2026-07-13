/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**", /\.toml$/, /\.log$/],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    // Tailwind v4 emits modern CSS (@layer, @property, color-mix()) that needs
    // Chrome 111+ / Safari 16.4+, so the target floor is raised to match. See
    // the macOS minimumSystemVersion (13.3) in src-tauri/tauri.conf.json.
    target:
      process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome111" : "safari16.4",
    minify: !process.env.TAURI_ENV_DEBUG ? "oxc" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: [],
  },
}));
