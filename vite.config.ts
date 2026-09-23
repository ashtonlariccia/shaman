import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  root: "ui",
  plugins: [svelte()],

  // Tauri owns the terminal output; don't let Vite wipe it.
  clearScreen: false,

  server: {
    port: 5173,
    strictPort: true,
    // The project lives on an NTFS mount edited from WSL, so native FS events
    // are unreliable here. Polling costs a little CPU but keeps HMR honest.
    watch: { usePolling: true, interval: 300 },
  },

  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // WebView2 is evergreen Chromium; no need to down-level far.
    target: "chrome110",
    sourcemap: false,
  },
});
