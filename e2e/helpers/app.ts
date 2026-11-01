import { spawn, type ChildProcess } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
} from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { bin } from "./paths.ts";
import { run } from "./run.ts";
import { shared } from "./state.ts";

/**
 * tauri-driver listens here; it starts WebKitWebDriver on the next port. Parallel runs
 * (`MUMCORD_E2E_RUN`, e2e/run.sh) share the host network and need ports of their own.
 */
export const driverPort = 4444 + 2 * Number(process.env.MUMCORD_E2E_RUN ?? 0);

export interface LaunchOptions {
  /**
   * Audio backend (`MUMCORD_AUDIO_BACKEND`): `"null"` (default, silence) or
   * `"file:<in.wav>:<out.wav>"` (mic plays in.wav, speakers record to out.wav).
   */
  audio?: string;
  /** Start with a new, empty profile (data dir + keyring). Default: keep the current one. */
  freshProfile?: boolean;
  /**
   * Extra environment for the app (e.g. a local update server). Replaces the extras of earlier
   * launches; default: keep them. Every spec file starts without extras.
   */
  env?: Record<string, string>;
}

const noExtraEnv: Record<string, string> = {};

// Shared with the wdio config (see state.ts): the hook starts the driver, the specs read it.
const state = shared("app", {
  driver: undefined as ChildProcess | undefined,
  profileDir: "",
  audio: "null",
  env: noExtraEnv,
});

/** The current profile's root (XDG data/config/cache dirs live below it). */
export function profile(): string {
  return state.profileDir;
}

/** Deletes Mumcord's keyring entries (identity, server passwords) from the Secret Service. */
async function clearKeyring(): Promise<void> {
  const result = await run("secret-tool", ["clear", "service", "app.mumcord.client"]);
  // Exit 1 just means nothing matched.
  if (result.code > 1) throw new Error(`secret-tool clear failed: ${result.stderr}`);
}

function newProfile(): void {
  state.profileDir = mkdtempSync(join(tmpdir(), "mumcord-profile-"));
  for (const d of ["data", "config", "cache"]) mkdirSync(join(state.profileDir, d));
}

function portOpen(port: number): Promise<boolean> {
  return new Promise((resolvePromise) => {
    const socket = createConnection({ port, host: "127.0.0.1" });
    socket.once("connect", () => {
      socket.end();
      resolvePromise(true);
    });
    socket.once("error", () => {
      resolvePromise(false);
    });
  });
}

async function waitPort(port: number, open: boolean): Promise<void> {
  for (let i = 0; i < 100; i++) {
    if ((await portOpen(port)) === open) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`port ${String(port)} did not ${open ? "open" : "close"}`);
}

/**
 * Starts tauri-driver. The app inherits its environment from the driver, so profile dirs, the
 * audio backend and extra env are fixed per driver process.
 */
export async function startDriver(): Promise<void> {
  if (!state.profileDir) newProfile();
  const ports = ["--port", String(driverPort), "--native-port", String(driverPort + 1)];
  state.driver = spawn("tauri-driver", ports, {
    stdio: ["ignore", "inherit", "inherit"],
    env: {
      ...process.env,
      XDG_DATA_HOME: join(state.profileDir, "data"),
      XDG_CONFIG_HOME: join(state.profileDir, "config"),
      XDG_CACHE_HOME: join(state.profileDir, "cache"),
      MUMCORD_AUDIO_BACKEND: state.audio,
      // Link previews may only fetch the local test page, never the internet (FR-1.49).
      MUMCORD_TEST_PREVIEW_ADDRESSES: "127.0.0.1",
      ...state.env,
    },
  });
  await waitPort(driverPort, true);
}

/** PIDs of Mumcord app processes and of the driver processes (tauri-driver, WebKitWebDriver). */
function strayPids(): { app: number[]; drivers: number[] } {
  let appExe = bin.app;
  try {
    appExe = realpathSync(bin.app);
  } catch {
    // Not built yet: nothing can be running.
  }
  const found = { app: [] as number[], drivers: [] as number[] };
  for (const name of readdirSync("/proc")) {
    const pid = Number(name);
    if (!Number.isInteger(pid) || pid === process.pid) continue;
    try {
      const comm = readFileSync(`/proc/${name}/comm`, "utf8").trim();
      if (comm === "tauri-driver" || comm === "WebKitWebDriver") found.drivers.push(pid);
      else if (readlinkSync(`/proc/${name}/exe`).replace(/ \(deleted\)$/, "") === appExe)
        found.app.push(pid);
    } catch {
      // Process gone or not ours.
    }
  }
  return found;
}

function alive(pid: number): boolean {
  try {
    // Zombies (exited, not yet reaped by --init) count as gone.
    return !/^\d+ \(.*\) Z/.test(readFileSync(`/proc/${String(pid)}/stat`, "utf8"));
  } catch {
    return false;
  }
}

/**
 * Makes sure no app or driver process survives. The app is single-instance (D-Bus name): a
 * leftover instance makes every later launch hand over to it and exit at once, so WebKitWebDriver
 * waits forever for its browser and `POST /session` hangs – for this spec and all later ones.
 * Leftovers happen when the driver is killed before the app finished quitting (slow CI runners).
 */
export async function killStrays(graceMs = 5_000): Promise<void> {
  const deadline = Date.now() + graceMs;
  let pids = strayPids();
  // Give a quitting app a moment to exit on its own before signalling it.
  while (pids.app.length && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    pids = strayPids();
  }
  const all = [...pids.app, ...pids.drivers].filter(alive);
  if (!all.length) return;
  if (pids.app.some(alive))
    console.warn(
      `[e2e] app still running after its session ended (pids ${pids.app.join(",")}); killing it`,
    );
  for (const sig of ["SIGTERM", "SIGKILL"] as const) {
    for (const pid of all) {
      try {
        process.kill(pid, sig);
      } catch {
        // Already gone.
      }
    }
    for (let i = 0; i < 30 && all.some(alive); i++) await new Promise((r) => setTimeout(r, 100));
    if (!all.some(alive)) return;
  }
  throw new Error(`could not kill leftover processes ${all.filter(alive).join(",")}`);
}

export async function stopDriver(): Promise<void> {
  state.driver?.kill("SIGTERM");
  state.driver = undefined;
  // A wedged driver may not close its port; killStrays below kills it either way.
  await waitPort(driverPort, false).catch(() => undefined);
  await killStrays();
  await waitPort(driverPort, false);
  await waitPort(driverPort + 1, false);
}

/** Prepares a fresh install for a new spec file (called from wdio.conf.ts before the session). */
export async function prepareFresh(): Promise<void> {
  newProfile();
  state.audio = "null";
  state.env = {};
  await killStrays(0);
  await clearKeyring();
  await startDriver();
}

/**
 * Quits the app and starts it again ("restart Mumcord"), optionally with a fresh profile or
 * another audio backend. Keeps the profile by default, so persisted state must survive.
 */
export async function restart(opts: LaunchOptions = {}): Promise<void> {
  // A broken session (an earlier test failed or timed out) must not stop the relaunch.
  await withTimeout(browser.deleteSession(), 15_000, "deleteSession").catch((e: unknown) => {
    console.warn(`[e2e] restart: ending the old session failed: ${String(e)}`);
  });
  await stopDriver();
  if (opts.freshProfile) {
    newProfile();
    await clearKeyring();
  }
  if (opts.audio !== undefined) state.audio = opts.audio;
  if (opts.env !== undefined) state.env = opts.env;
  await startDriver();
  // A healthy debug build starts in seconds even on a 2-core runner; fail clearly instead of
  // letting the test hit the mocha timeout.
  await withTimeout(browser.reloadSession(), 60_000, "new session (app did not start)");
}

/** Rejects with a clear message when `p` takes longer than `ms`. */
export function withTimeout<T>(p: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const t = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${what}: no answer within ${String(ms / 1000)} s`));
    }, ms);
  });
  return Promise.race([p, t]).finally(() => {
    clearTimeout(timer);
  });
}

/**
 * Calls a Tauri command of the app in the current window and returns its result, for state the
 * UI does not show (or to switch features without their own UI, like the overlay).
 */
export async function invoke<T>(command: string, args: object = {}): Promise<T> {
  return browser.execute(
    async (c: string, a: object) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: object) => Promise<unknown> };
      };
      return w.__TAURI_INTERNALS__.invoke(c, a);
    },
    command,
    args,
  ) as Promise<T>;
}

/**
 * Switches the app to the classic sound theme (#325). Specs that measure the speaker output were
 * tuned to its short tones; the default rich sounds last up to 1.2 s and would spill into their
 * measurement windows. Stored in the profile, so it survives restarts that keep the profile.
 */
export async function useClassicSounds(): Promise<void> {
  const settings = await invoke<Record<string, unknown>>("settings_get");
  await invoke("settings_update", { settings: { ...settings, soundTheme: "classic" } });
}
