// A second Mumcord client ("B") next to the wdio session of every spec ("A"), for criteria that
// need two Mumcord users (plugin messaging, blocking, ...). B is its own app process with its own
// profile, keyring and WebDriver session; see e2e/README.md "Two clients".
import { remote } from "webdriverio";
import {
  applyLaunchOptions,
  capabilities,
  instance,
  startDriver,
  stopDriver,
  withTimeout,
  type LaunchOptions,
} from "./app.ts";
import { shared } from "./state.ts";

const b = instance("b");

const session = shared("appB", { browser: undefined as WebdriverIO.Browser | undefined });

/** The WebDriver session of instance B; throws when B is not running. */
export function browserB(): WebdriverIO.Browser {
  if (!session.browser) throw new Error("instance B is not running (call startB first)");
  return session.browser;
}

/** Whether instance B has a session (for the failure artifacts in wdio.conf.ts). */
export function runningB(): boolean {
  return session.browser !== undefined;
}

async function openSession(): Promise<void> {
  await startDriver(b);
  session.browser = await withTimeout(
    remote({
      hostname: "127.0.0.1",
      port: b.port,
      capabilities: capabilities(b),
      logLevel: "warn",
      waitforTimeout: 15_000,
      connectionRetryTimeout: 120_000,
      connectionRetryCount: 1,
    }),
    60_000,
    "instance B: new session (app did not start)",
  );
}

async function closeSession(): Promise<void> {
  const current = session.browser;
  session.browser = undefined;
  if (current) {
    // A broken session (an earlier test failed or timed out) must not stop the cleanup.
    await withTimeout(current.deleteSession(), 15_000, "instance B: deleteSession").catch(
      (e: unknown) => {
        console.warn(`[e2e] ending the session of instance B failed: ${String(e)}`);
      },
    );
  }
  await stopDriver(b);
}

/**
 * Starts instance B. It starts with a fresh profile (new identity, empty keyring) unless
 * `freshProfile: false` is given and B ran before in this spec file. Other options like
 * `restart` in app.ts.
 */
export async function startB(opts: LaunchOptions = {}): Promise<void> {
  if (session.browser) throw new Error("instance B is already running");
  await applyLaunchOptions(b, { ...opts, freshProfile: opts.freshProfile ?? true });
  await openSession();
}

/** Quits app B and starts it again; keeps its profile (and identity) by default. */
export async function restartB(opts: LaunchOptions = {}): Promise<void> {
  await closeSession();
  await applyLaunchOptions(b, opts);
  await openSession();
}

/** Quits app B (no-op when it is not running). */
export async function stopB(): Promise<void> {
  if (!session.browser && !b.driver) return;
  await closeSession();
}

let insideB = false;

/**
 * Runs `fn` with the wdio globals (`browser`, `driver`, `$`, `$$`) pointed at instance B, so every
 * UI helper written for the normal session drives B. A and B steps must stay sequential: never
 * put an `onB` call and A commands into one `Promise.all`.
 */
export async function onB<T>(fn: () => Promise<T>): Promise<T> {
  if (insideB) return fn();
  const target = browserB();
  // @wdio/globals resolves its exports from this map on every access (see its proxyHandler).
  const globals = (globalThis as unknown as { _wdioGlobals: Map<string, unknown> })._wdioGlobals;
  const saved = new Map(["browser", "driver", "$", "$$"].map((k) => [k, globals.get(k)]));
  globals.set("browser", target);
  globals.set("driver", target);
  globals.set("$", target.$.bind(target));
  globals.set("$$", target.$$.bind(target));
  insideB = true;
  try {
    return await fn();
  } finally {
    insideB = false;
    for (const [k, v] of saved) globals.set(k, v);
  }
}

/** Test settings for the plugin system, turned into app environment by `pluginEnv`. */
export interface PluginTestEnv {
  /** Public minisign key of the run's fixture registry (debug builds only, FR-6.53). */
  registryKey?: string;
  /** Base URL of the fixture registry (`mumcord-plugin serve-registry`). */
  registryUrl?: string;
  /** Fake app version, to trigger the once-per-version re-hash of installed plugins (AC-6.21). */
  fakeAppVersion?: string;
  /** Run the daily revocation and update check at start (AC-6.20). */
  forceDailyCheck?: boolean;
  /** Shorten the crash back-off timers (AC-6.8). */
  fastBackoff?: boolean;
}

/**
 * Environment for `restart`/`startB`/`restartB` (`env` option) that points the plugin system at
 * test fixtures. Debug builds only; release builds ignore these variables.
 */
export function pluginEnv(t: PluginTestEnv): Record<string, string> {
  const env: Record<string, string> = {};
  if (t.registryKey !== undefined) env.MUMCORD_PLUGIN_REGISTRY_KEY = t.registryKey;
  if (t.registryUrl !== undefined) env.MUMCORD_PLUGIN_REGISTRY_URL = t.registryUrl;
  if (t.fakeAppVersion !== undefined) env.MUMCORD_PLUGIN_TEST_APP_VERSION = t.fakeAppVersion;
  if (t.forceDailyCheck) env.MUMCORD_PLUGIN_TEST_FORCE_DAILY_CHECK = "1";
  if (t.fastBackoff) env.MUMCORD_PLUGIN_TEST_FAST_BACKOFF = "1";
  return env;
}
