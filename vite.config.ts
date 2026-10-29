import { readFileSync } from "node:fs";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { parseChangelog } from "./ui/src/features/changelog/parseChangelog";

const CHANGELOG = new URL("./CHANGELOG.md", import.meta.url);
const CHANGELOG_FILE = "changelog.json";

function changelogJson(): string {
  return JSON.stringify(parseChangelog(readFileSync(CHANGELOG, "utf8")));
}

/**
 * Serves CHANGELOG.md as changelog.json for the "What's new" window (issue #145): generated on
 * every build, so the shipped app always matches the changelog and needs no network.
 */
function changelog(): Plugin {
  return {
    name: "mumcord-changelog",
    configureServer(server) {
      server.middlewares.use(`/${CHANGELOG_FILE}`, (_request, response) => {
        response.setHeader("Content-Type", "application/json");
        response.end(changelogJson());
      });
    },
    generateBundle() {
      this.emitFile({ type: "asset", fileName: CHANGELOG_FILE, source: changelogJson() });
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
