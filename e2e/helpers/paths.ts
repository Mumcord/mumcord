import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");

/** Debug binaries built by container-run.sh (mumbot is the pinned release it fetches). */
export const bin = {
  app: join(targetDir, "debug", "mumcord"),
  bot: join(targetDir, "debug", "mumbot"),
  netsim: join(targetDir, "debug", "netsim"),
  /** Mumble game plugin for tests (crates/positional/examples/test_plugin.rs). */
  testPlugin: join(targetDir, "debug", "examples", "libtest_plugin.so"),
  /** Renders frames through the in-game overlay layer (crates/overlay-layer/examples). */
  headlessCheck: join(targetDir, "debug", "examples", "headless_check"),
};

export const testenvDir = join(repoRoot, "testenv");
/** Screenshots, page sources and axe reports (git-ignored). */
export const artifactsDir = join(repoRoot, "e2e", "artifacts");
