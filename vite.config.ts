import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and serves the built UI from ../dist.
export default defineConfig({
  root: "ui",
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
    // Two pages: the main window and the overlay window (FR-1.44).
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./ui/index.html", import.meta.url)),
        overlay: fileURLToPath(new URL("./ui/overlay.html", import.meta.url)),
      },
    },
  },
});
