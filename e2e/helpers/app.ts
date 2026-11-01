import { spawn, type ChildProcess } from "node:child_process";
import { once } from "node:events";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
} from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { bin } from "./paths.ts";
import { run } from "./run.ts";
import { shared } from "./state.ts";

/** Which app instance: `a` is the wdio session every spec has, `b` the optional second client. */
export type AppId = "a" | "b";

/**
 * tauri-driver of instance A listens here; it starts WebKitWebDriver on the next port. Parallel
 * runs (`MUMCORD_E2E_RUN`, e2e/run.sh) share the host network and need ports of their own.
 */
export const driverPort = 4444 + 2 * Number(process.env.MUMCORD_E2E_RUN ?? 0);
/** Instance B's tauri-driver: above every port instance A of runs 0-9 can take. */
const driverPortB = 4544 + 2 * Number(process.env.MUMCORD_E2E_RUN ?? 0);

/**
 * Marks every process an instance starts (driver, app, private D-Bus, keyring), so cleaning up one
 * instance never kills the other. Processes without it (e.g. an app a spec launched itself)
 * count as instance A.
 */
const instanceMarker = "MUMCORD_E2E_INSTANCE";

export interface LaunchOptions {
  /**
   * Audio backend (`MUMCORD_AUDIO_BACKEND`): `"null"` (default, silence) or
   * `"file:<in.wav>:<out.wav>"` (mic plays in.wav, speakers record to out.wav).
   */
  audio?: string;
  /** Start with a new, empty profile (data dir + keyring). Default: keep the current one. */
  freshProfile?: boolean;
  /**
   * Extra environment for the app (e.g. a local update server, see `pluginEnv` in apps.ts).
   * Replaces the extras of earlier launches; default: keep them. Every spec file starts without
   * extras.
   */
  env?: Record<string, string>;
  /**
   * Command-line arguments for the app (e.g. `["--no-plugins"]`). Replace the arguments of
   * earlier launches; default: keep them. Every spec file starts without arguments.
   */
  args?: string[];
}

/**
 * Instance B's own session D-Bus with an unlocked Secret Service. The app's single-instance lock
 * is a D-Bus name and its secrets live in the Secret Service, so a second app on the shared bus
 * would hand over to the first and exit, or share its identity.
 */
interface PrivateBus {
  address: string;
  runtimeDir: string;
  daemon: ChildProcess;
  keyring: ChildProcess;
}

interface Instance {
  id: AppId;
  port: number;
  driver: ChildProcess | undefined;
  profileDir: string;
  audio: string;
  env: Record<string, string>;
  args: string[];
  bus: PrivateBus | undefined;
}

function newInstance(id: AppId, port: number): Instance {
  return {
    id,
    port,
    driver: undefined,
    profileDir: "",
    audio: "null",
    env: {},
    args: [],
    bus: undefined,
  };
}

// Shared with the wdio config (see state.ts): the hook starts the driver, the specs read it.
const state = shared("app", {
  a: newInstance("a", driverPort),
  b: newInstance("b", driverPortB),
});

/** State of one instance (for apps.ts). */
export function instance(id: AppId): Instance {
  return state[id];
}

/** The profile root of an instance (XDG data/config/cache dirs live below it). Default: A. */
export function profile(id: AppId = "a"): string {
  return state[id].profileDir;
}

/** Deletes Mumcord's keyring entries (identity, server passwords) from instance A's Secret Service. */
async function clearKeyring(): Promise<void> {
  const result = await run("secret-tool", ["clear", "service", "app.mumcord.client"]);
  // Exit 1 just means nothing matched.
  if (result.code > 1) throw new Error(`secret-tool clear failed: ${result.stderr}`);
}

/**
 * Gives the instance a new, empty profile. Instance B's keyring file lives in its profile (its
 * keyring daemon uses the profile's data dir), so a new profile is a new keyring too; instance A
 * shares the session keyring and clears it.
 */
export async function newProfile(inst: Instance = state.a): Promise<void> {
  inst.profileDir = mkdtempSync(join(tmpdir(), `mumcord-profile-${inst.id}-`));
  for (const d of ["data", "config", "cache"]) mkdirSync(join(inst.profileDir, d));
  if (inst.id === "a") await clearKeyring();
}

/** The WebDriver capabilities that launch the app binary with the instance's arguments. */
export function capabilities(inst: Instance = state.a): WebdriverIO.Capabilities {
  // tauri-driver launches this binary through WebKitWebDriver.
  return {
    "tauri:options": { application: bin.app, args: inst.args },
  } as WebdriverIO.Capabilities;
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

/** Resolves once `org.freedesktop.secrets` is owned on the bus (the keyring daemon is ready). */
async function waitSecretService(address: string): Promise<void> {
  const args = [
    "--session",
    "--print-reply",
    "--dest=org.freedesktop.DBus",
    "/org/freedesktop/DBus",
    "org.freedesktop.DBus.NameHasOwner",
    "string:org.freedesktop.secrets",
  ];
  for (let i = 0; i < 100; i++) {
    const r = await run("dbus-send", args, { DBUS_SESSION_BUS_ADDRESS: address });
    if (r.stdout.includes("boolean true")) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error("instance B: the Secret Service did not start on its private bus");
}

/**
 * Starts instance B's private session bus and an unlocked keyring on it. The keyring file lives
 * in B's profile, so a restart with the same profile finds the same identity again.
 */
async function startPrivateBus(inst: Instance): Promise<PrivateBus> {
  const runtimeDir = mkdtempSync(join(tmpdir(), `mumcord-runtime-${inst.id}-`));
  const env = {
    ...process.env,
    [instanceMarker]: inst.id,
    XDG_RUNTIME_DIR: runtimeDir,
    XDG_DATA_HOME: join(inst.profileDir, "data"),
  };
  const daemon = spawn("dbus-daemon", ["--session", "--nofork", "--print-address=1"], {
    env,
    stdio: ["ignore", "pipe", "inherit"],
  });
  const [line] = (await withTimeout(once(daemon.stdout, "data"), 10_000, "private D-Bus")) as [
    Buffer,
  ];
  const address = line.toString().split("\n")[0].trim();
  // Same fixed password as container-run.sh: a non-empty one creates and unlocks the login
  // keyring without a prompt.
  const keyring = spawn(
    "gnome-keyring-daemon",
    ["--foreground", "--unlock", "--components=secrets"],
    { env: { ...env, DBUS_SESSION_BUS_ADDRESS: address }, stdio: ["pipe", "ignore", "ignore"] },
  );
  keyring.stdin.end("e2e");
  await waitSecretService(address);
  return { address, runtimeDir, daemon, keyring };
}

function stopPrivateBus(inst: Instance): void {
  if (!inst.bus) return;
  inst.bus.keyring.kill("SIGTERM");
  inst.bus.daemon.kill("SIGTERM");
  rmSync(inst.bus.runtimeDir, { recursive: true, force: true });
  inst.bus = undefined;
}

/**
 * Starts the instance's tauri-driver (default: A). The app inherits its environment from the
 * driver, so profile dirs, the audio backend and extra env are fixed per driver process.
 */
export async function startDriver(inst: Instance = state.a): Promise<void> {
  if (!inst.profileDir) await newProfile(inst);
  const ports = ["--port", String(inst.port), "--native-port", String(inst.port + 1)];
  let own: NodeJS.ProcessEnv = {};
  if (inst.id === "b") {
    inst.bus = await startPrivateBus(inst);
    own = {
      DBUS_SESSION_BUS_ADDRESS: inst.bus.address,
      XDG_RUNTIME_DIR: inst.bus.runtimeDir,
      // Abstract socket names are per network namespace, so B needs one apart from A's.
      MUMCORD_OVERLAY_SOCKET: `${process.env.MUMCORD_OVERLAY_SOCKET ?? "mumcord-overlay-e2e"}-b`,
    };
  }
  inst.driver = spawn("tauri-driver", ports, {
    stdio: ["ignore", "inherit", "inherit"],
    env: {
      ...process.env,
      ...own,
      [instanceMarker]: inst.id,
      XDG_DATA_HOME: join(inst.profileDir, "data"),
      XDG_CONFIG_HOME: join(inst.profileDir, "config"),
      XDG_CACHE_HOME: join(inst.profileDir, "cache"),
      MUMCORD_AUDIO_BACKEND: inst.audio,
      // Link previews may only fetch the local test page, never the internet (FR-1.49).
      MUMCORD_TEST_PREVIEW_ADDRESSES: "127.0.0.1",
      ...inst.env,
    },
  });
  await waitPort(inst.port, true);
}

/** The instance a process belongs to, from its environment (unmarked = A). */
function processInstance(pid: string): AppId {
  const environ = readFileSync(`/proc/${pid}/environ`, "utf8");
  return `\0${environ}`.includes(`\0${instanceMarker}=b\0`) ? "b" : "a";
}

/**
 * PIDs of Mumcord app processes and of the driver processes (tauri-driver, WebKitWebDriver) of
 * one instance; for B also its private bus and keyring daemon.
 */
function strayPids(id: AppId): { app: number[]; drivers: number[] } {
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
      if (processInstance(name) !== id) continue;
      const comm = readFileSync(`/proc/${name}/comm`, "utf8").trim();
      const helper =
        comm === "tauri-driver" ||
        comm === "WebKitWebDriver" ||
        (id === "b" && (comm === "dbus-daemon" || comm === "gnome-keyring-d"));
      if (helper) found.drivers.push(pid);
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
 * Makes sure no app or driver process of the instance survives (default: A). The app is
 * single-instance (D-Bus name): a leftover instance makes every later launch on that bus hand
 * over to it and exit at once, so WebKitWebDriver waits forever for its browser and
 * `POST /session` hangs – for this spec and all later ones. Leftovers happen when the driver is
 * killed before the app finished quitting (slow CI runners).
 */
export async function killStrays(graceMs = 5_000, id: AppId = "a"): Promise<void> {
  const deadline = Date.now() + graceMs;
  let pids = strayPids(id);
  // Give a quitting app a moment to exit on its own before signalling it.
  while (pids.app.length && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 100));
    pids = strayPids(id);
  }
  const all = [...pids.app, ...pids.drivers].filter(alive);
  if (!all.length) return;
  if (pids.app.some(alive))
    console.warn(
      `[e2e] app ${id.toUpperCase()} still running after its session ended (pids ${pids.app.join(",")}); killing it`,
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

/** Stops the instance's driver and app (default: A); for B also its private bus. */
export async function stopDriver(inst: Instance = state.a): Promise<void> {
  inst.driver?.kill("SIGTERM");
  inst.driver = undefined;
  // A wedged driver may not close its port; killStrays below kills it either way.
  await waitPort(inst.port, false).catch(() => undefined);
  await killStrays(5_000, inst.id);
  stopPrivateBus(inst);
  await waitPort(inst.port, false);
  await waitPort(inst.port + 1, false);
}

/**
 * Prepares a fresh install for a new spec file (called from wdio.conf.ts before the session):
 * instance A with a new profile and no extras, and no instance B left from an earlier spec.
 */
export async function prepareFresh(): Promise<void> {
  state.a = newInstance("a", driverPort);
  state.b = newInstance("b", driverPortB);
  await killStrays(0, "b");
  await killStrays(0, "a");
  await newProfile(state.a);
  await startDriver(state.a);
}

/** Applies launch options to an instance before its driver starts again. */
export async function applyLaunchOptions(inst: Instance, opts: LaunchOptions): Promise<void> {
  if (opts.freshProfile) await newProfile(inst);
  if (opts.audio !== undefined) inst.audio = opts.audio;
  if (opts.env !== undefined) inst.env = opts.env;
  if (opts.args !== undefined) inst.args = opts.args;
}

/**
 * Quits app A and starts it again ("restart Mumcord"), optionally with a fresh profile, another
 * audio backend, env or arguments. Keeps the profile by default, so persisted state must
 * survive. Instance B keeps running; use `restartB` (apps.ts) for it.
 */
export async function restart(opts: LaunchOptions = {}): Promise<void> {
  // A broken session (an earlier test failed or timed out) must not stop the relaunch.
  await withTimeout(browser.deleteSession(), 15_000, "deleteSession").catch((e: unknown) => {
    console.warn(`[e2e] restart: ending the old session failed: ${String(e)}`);
  });
  await stopDriver(state.a);
  await applyLaunchOptions(state.a, opts);
  await startDriver(state.a);
  // A healthy debug build starts in seconds even on a 2-core runner; fail clearly instead of
  // letting the test hit the mocha timeout.
  await withTimeout(
    browser.reloadSession(capabilities(state.a)),
    60_000,
    "new session (app did not start)",
  );
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
