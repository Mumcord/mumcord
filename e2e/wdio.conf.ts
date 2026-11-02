import { globSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { browser } from "@wdio/globals";
import { capabilities, driverPort, prepareFresh, stopDriver, withTimeout } from "./helpers/app.ts";
import { browserB, runningB, stopB } from "./helpers/apps.ts";
import { netsim } from "./helpers/index.ts";
import { artifactsDir } from "./helpers/paths.ts";

const e2eDir = dirname(fileURLToPath(import.meta.url));
const allSpecs = "specs/**/*.e2e.ts";

/**
 * Spec files of one shard (1-based) from shards.json, so CI can split the suite over parallel
 * jobs. Fails when the file and the specs on disk disagree: a new spec missing from every shard
 * would otherwise silently never run in CI.
 */
function shardSpecs(shard: string): string[] {
  const { shards } = JSON.parse(readFileSync(join(e2eDir, "shards.json"), "utf8")) as {
    shards: string[][];
  };
  const listed = shards.flat();
  const onDisk = globSync(allSpecs, { cwd: e2eDir }).sort();
  const missing = onDisk.filter((f) => !listed.includes(f));
  const stale = listed.filter((f, i) => !onDisk.includes(f) || listed.indexOf(f) !== i);
  if (missing.length > 0 || stale.length > 0) {
    throw new Error(
      `e2e/shards.json is out of date. Not in any shard: [${missing.join(", ")}]; ` +
        `unknown or duplicate: [${stale.join(", ")}]`,
    );
  }
  const index = Number(shard);
  if (!Number.isInteger(index) || index < 1 || index > shards.length) {
    throw new Error(`MUMCORD_E2E_SHARD must be 1..${String(shards.length)}, got "${shard}"`);
  }
  return shards[index - 1].map((f) => `./${f}`);
}

const shard = process.env.MUMCORD_E2E_SHARD;

// Each spec file = one worker = one fresh install (new profile, empty keyring, and no app or
// driver process left over from the previous spec – see killStrays). Specs run one at a
// time because they share the test servers, the keyring and the netsim ports.
export const config: WebdriverIO.Config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: driverPort,
  specs: shard ? shardSpecs(shard) : [`./${allSpecs}`],
  maxInstances: 1,
  capabilities: [capabilities()],
  logLevel: "warn",
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 180_000 },
  reporters: ["spec"],

  beforeSession: async () => {
    await prepareFresh();
  },

  afterTest: async (test, _context, { passed }) => {
    if (passed) return;
    mkdirSync(artifactsDir, { recursive: true });
    const name = `${test.parent} ${test.title}`.replace(/[^\w-]+/g, "_").slice(0, 120);
    // Best effort: after a hang the session may be dead, and every command would wait for
    // connectionRetryTimeout.
    const sessions: [string, WebdriverIO.Browser][] = [[name, browser]];
    if (runningB()) sessions.push([`${name}-B`, browserB()]);
    for (const [file, session] of sessions) {
      try {
        await withTimeout(
          session.saveScreenshot(join(artifactsDir, `${file}.png`)),
          10_000,
          "screenshot",
        );
        writeFileSync(
          join(artifactsDir, `${file}.html`),
          await withTimeout(session.getPageSource(), 10_000, "page source"),
        );
      } catch (e) {
        console.warn(`[e2e] no failure screenshot/page source for ${file}: ${String(e)}`);
      }
    }
  },

  afterSession: async () => {
    // Instance A must be stopped even when B's cleanup fails.
    await stopB().catch((e: unknown) => {
      console.warn(`[e2e] stopping instance B failed: ${String(e)}`);
    });
    netsim.stop();
    await stopDriver();
  },
};
