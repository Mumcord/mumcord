import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");

/** Debug binaries built by container-run.sh. */
export const bin = {
  app: join(targetDir, "debug", "mumcord"),
  bot: join(targetDir, "debug", "mumcord-bot"),
  netsim: join(targetDir, "debug", "netsim"),
};

export const testenvDir = join(repoRoot, "testenv");
/** Screenshots, page sources and axe reports (git-ignored). */
export const artifactsDir = join(repoRoot, "e2e", "artifacts");
