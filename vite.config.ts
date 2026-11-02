import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { parseChangelog } from "./ui/src/features/changelog/parseChangelog";

const CHANGELOG = new URL("./CHANGELOG.md", import.meta.url);
const VIRTUAL_ID = "virtual:changelog";
const RESOLVED_ID = `\0${VIRTUAL_ID}`;

/**
 * Provides the parsed CHANGELOG.md as the module `virtual:changelog` for the "What's new" window
 * (issue #145). It lands in the JS bundle, so the window needs no request, and every build reads
 * the CHANGELOG.md of the commit it is built from.
 */
function changelog(): Plugin {
  return {
    name: "mumcord-changelog",
    resolveId(id) {
      return id === VIRTUAL_ID ? RESOLVED_ID : null;
    },
    load(id) {
      if (id !== RESOLVED_ID) return null;
      this.addWatchFile(fileURLToPath(CHANGELOG));
      const versions = parseChangelog(readFileSync(CHANGELOG, "utf8"));
      return `export default ${JSON.stringify(versions)};`;
    },
  };
}

// Tauri expects a fixed dev port and serves the built UI from ../dist.
export default defineConfig({
  root: "ui",
  plugins: [react(), changelog()],
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
  },
});
