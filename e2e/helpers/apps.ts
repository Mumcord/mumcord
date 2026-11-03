// A second and third Mumcord client ("B", "C") next to the wdio session of every spec ("A"), for
// criteria that need two or three Mumcord users (plugin messaging, blocking, file sharing, ...).
// Each is its own app process with its own profile, keyring and WebDriver session; see
// e2e/README.md "Two clients".
import { remote } from "webdriverio";
import {
  applyLaunchOptions,
  capabilities,
  instance,
  startDriver,
  stopDriver,
  withTimeout,
  type AppId,
  type LaunchOptions,
} from "./app.ts";
import { shared } from "./state.ts";

/** Start, restart, stop and the WebDriver session of one extra client (B or C). */
function extraClient(id: Exclude<AppId, "a">) {
  const label = id.toUpperCase();
  // Looked up on every use: prepareFresh() gives each spec file new instance objects.
  const inst = (): ReturnType<typeof instance> => instance(id);
  const session = shared(`app${label}`, { browser: undefined as WebdriverIO.Browser | undefined });

  function browserOf(): WebdriverIO.Browser {
    if (!session.browser)
      throw new Error(`instance ${label} is not running (call start${label} first)`);
    return session.browser;
  }

  async function openSession(): Promise<void> {
    await startDriver(inst());
    session.browser = await withTimeout(
      remote({
        hostname: "127.0.0.1",
        port: inst().port,
        capabilities: capabilities(inst()),
        logLevel: "warn",
        waitforTimeout: 15_000,
        connectionRetryTimeout: 120_000,
        connectionRetryCount: 1,
      }),
      60_000,
      `instance ${label}: new session (app did not start)`,
    );
  }

  async function closeSession(): Promise<void> {
    const current = session.browser;
    session.browser = undefined;
    if (current) {
      // A broken session (an earlier test failed or timed out) must not stop the cleanup.
      await withTimeout(current.deleteSession(), 15_000, `instance ${label}: deleteSession`).catch(
        (e: unknown) => {
          console.warn(`[e2e] ending the session of instance ${label} failed: ${String(e)}`);
        },
      );
    }
    await stopDriver(inst());
  }

  return {
    browser: browserOf,
    running: (): boolean => session.browser !== undefined,
    start: async (opts: LaunchOptions = {}): Promise<void> => {
      if (session.browser) throw new Error(`instance ${label} is already running`);
      await applyLaunchOptions(inst(), { ...opts, freshProfile: opts.freshProfile ?? true });
      await openSession();
    },
    restart: async (opts: LaunchOptions = {}): Promise<void> => {
      await closeSession();
      await applyLaunchOptions(inst(), opts);
      await openSession();
    },
    stop: async (): Promise<void> => {
      if (!session.browser && !inst().driver) return;
      await closeSession();
    },
  };
}

const clientB = extraClient("b");
const clientC = extraClient("c");

/** The WebDriver session of instance B; throws when B is not running. */
export const browserB = clientB.browser;
/** The WebDriver session of instance C; throws when C is not running. */
export const browserC = clientC.browser;

/** Whether instance B has a session (for the failure artifacts in wdio.conf.ts). */
export const runningB = clientB.running;
/** Whether instance C has a session (for the failure artifacts in wdio.conf.ts). */
export const runningC = clientC.running;

/**
 * Starts instance B. It starts with a fresh profile (new identity, empty keyring) unless
 * `freshProfile: false` is given and B ran before in this spec file. Other options like
 * `restart` in app.ts.
 */
export const startB = clientB.start;
/** Starts instance C like `startB`; use it for specs with three Mumcord users. */
export const startC = clientC.start;

/** Quits app B and starts it again; keeps its profile (and identity) by default. */
export const restartB = clientB.restart;
/** Quits app C and starts it again; keeps its profile (and identity) by default. */
export const restartC = clientC.restart;

/** Quits app B (no-op when it is not running). */
export const stopB = clientB.stop;
/** Quits app C (no-op when it is not running). */
export const stopC = clientC.stop;

/**
 * Runs `fn` with the wdio globals (`browser`, `driver`, `$`, `$$`) pointed at `target`, so every
 * UI helper written for the normal session drives that client. Nested calls restore the outer
 * target on the way out.
 */
async function onClient<T>(target: WebdriverIO.Browser, fn: () => Promise<T>): Promise<T> {
  // @wdio/globals resolves its exports from this map on every access (see its proxyHandler).
  const globals = (globalThis as unknown as { _wdioGlobals: Map<string, unknown> })._wdioGlobals;
  const saved = new Map(["browser", "driver", "$", "$$"].map((k) => [k, globals.get(k)]));
  globals.set("browser", target);
  globals.set("driver", target);
  globals.set("$", target.$.bind(target));
  globals.set("$$", target.$$.bind(target));
  try {
    return await fn();
  } finally {
    for (const [k, v] of saved) globals.set(k, v);
  }
}

/**
 * Runs `fn` against instance B. A, B and C steps must stay sequential: never put an `onB`/`onC`
 * call and commands of another client into one `Promise.all`.
 */
export const onB = <T>(fn: () => Promise<T>): Promise<T> => onClient(clientB.browser(), fn);
/** Runs `fn` against instance C; same rules as `onB`. */
export const onC = <T>(fn: () => Promise<T>): Promise<T> => onClient(clientC.browser(), fn);

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
