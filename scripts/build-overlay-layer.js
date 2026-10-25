// Builds the Linux in-game overlay library (crates/overlay-layer) before the app (FR-1.46b).
//
// Tauri runs this from `src-tauri/tauri.linux.conf.json` (beforeDevCommand / beforeBuildCommand),
// so it only runs on Linux. Cargo puts the library next to the app executable in the same target
// directory, which is where `npm run tauri dev` looks for it. A copy goes to
// `src-tauri/gen/overlay-layer/`, a fixed path the Linux bundles pick it up from, because the
// target directory itself moves with CARGO_TARGET_DIR.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// Tauri sets TAURI_ENV_DEBUG to "false" only for release builds; a manual run builds debug.
const release = process.env.TAURI_ENV_DEBUG === "false";

const args = [
  "build",
  "--package",
  "mumcord-overlay-layer",
  "--message-format=json-render-diagnostics",
];
if (release) args.push("--release");

const cargo = spawnSync("cargo", args, {
  cwd: root,
  encoding: "utf8",
  stdio: ["ignore", "pipe", "inherit"],
  maxBuffer: 64 * 1024 * 1024,
});
if (cargo.error) throw cargo.error;
if (cargo.status !== 0) {
  console.error("Building the in-game overlay library failed.");
  process.exit(cargo.status ?? 1);
}

const library = cargo.stdout
  .split("\n")
  .filter((line) => line.startsWith("{"))
  .map((line) => JSON.parse(line))
  .filter((message) => message.reason === "compiler-artifact")
  .filter((message) => message.target.name === "mumcord_overlay_layer")
  .flatMap((message) => message.filenames)
  .find((file) => file.endsWith(".so"));
if (library === undefined) {
  console.error("Cargo did not report the in-game overlay library (libmumcord_overlay_layer.so).");
  process.exit(1);
}

const staged = join(root, "src-tauri", "gen", "overlay-layer");
mkdirSync(staged, { recursive: true });
copyFileSync(library, join(staged, "libmumcord_overlay_layer.so"));
console.log(`In-game overlay library: ${library}`);
