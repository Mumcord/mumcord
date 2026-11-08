// Stages the ONNX Runtime library for the installers (spec 08, FR-8.33).
//
// crates/effects/build.rs downloads the pinned official build and checks its SHA-256; this
// script runs that build script (`cargo check` of the crate) for the bundle's target, takes the
// verified library path it reports (MUMCORD_ORT_BUILD_DYLIB) and copies the library to
// `src-tauri/gen/onnxruntime/`, a fixed path the bundle configs pick it up from:
//   src-tauri/tauri.onnxruntime.linux.json    AppImage, .deb, .rpm: /usr/lib/mumcord/
//   src-tauri/tauri.onnxruntime.windows.json  NSIS installer: next to Mumcord.exe
//   src-tauri/tauri.onnxruntime.macos.json    app bundle: Contents/Frameworks/
// These are separate from tauri.<platform>.conf.json because Tauri checks at compile time that
// Windows resources and macOS frameworks exist, which would break every plain cargo build.
//
// Usage: node scripts/stage-onnxruntime.js [target-triple]
// The target defaults to TAURI_ENV_TARGET_TRIPLE, then to the host.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    console.error(`${command} ${args.join(" ")} failed (exit code ${result.status}).`);
    process.exit(result.status ?? 1);
  }
  return result.stdout;
}

function hostTriple() {
  const host = run("rustc", ["-vV"])
    .split("\n")
    .find((line) => line.startsWith("host: "));
  if (host === undefined) {
    console.error("rustc -vV did not report the host target.");
    process.exit(1);
  }
  return host.slice("host: ".length).trim();
}

// The verified library of one target, as reported by the effects crate's build script.
function libraryFor(triple) {
  const output = run("cargo", [
    "check",
    "--package",
    "mumcord-effects",
    "--locked",
    "--target",
    triple,
    "--message-format=json-render-diagnostics",
  ]);
  const library = output
    .split("\n")
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line))
    .filter((message) => message.reason === "build-script-executed")
    .flatMap((message) => message.env)
    .find(([name]) => name === "MUMCORD_ORT_BUILD_DYLIB")?.[1];
  if (!library) {
    // build.rs printed the reason as a cargo warning above.
    console.error(
      `No ONNX Runtime library for ${triple}: the package would ship without background effects.`,
    );
    process.exit(1);
  }
  return library;
}

// The cache names files "<hash prefix>-<name>"; the bundle needs the plain name.
const plainName = (path) => basename(path).replace(/^[0-9a-f]{16}-/, "");

const triple = process.argv[2] ?? process.env.TAURI_ENV_TARGET_TRIPLE ?? hostTriple();
const staged = join(root, "src-tauri", "gen", "onnxruntime");
rmSync(staged, { recursive: true, force: true });
mkdirSync(staged, { recursive: true });

const library = libraryFor(triple);
const target = join(staged, plainName(library));
copyFileSync(library, target);
console.log(`ONNX Runtime (${triple}): ${library}`);
