import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, mkdirSync } from "node:fs";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import { run } from "./run.ts";

/** tauri-driver listens here; it starts WebKitWebDriver on the next port. */
export const driverPort = 4444;

export interface LaunchOptions {
  /**
   * Audio backend (`MUMCORD_AUDIO_BACKEND`): `"null"` (default, silence) or
   * `"file:<in.wav>:<out.wav>"` (mic plays in.wav, speakers record to out.wav).
   */
  audio?: string;
  /** Start with a new, empty profile (data dir + keyring). Default: keep the current one. */
  freshProfile?: boolean;
}

let driver: ChildProcess | undefined;
let profileDir = "";
let audio = "null";

/** The current profile's root (XDG data/config/cache dirs live below it). */
export function profile(): string {
  return profileDir;
}

/** Deletes Mumcord's keyring entries (identity, server passwords) from the Secret Service. */
async function clearKeyring(): Promise<void> {
  const result = await run("secret-tool", ["clear", "service", "app.mumcord.client"]);
  // Exit 1 just means nothing matched.
  if (result.code > 1) throw new Error(`secret-tool clear failed: ${result.stderr}`);
}

function newProfile(): void {
  profileDir = mkdtempSync(join(tmpdir(), "mumcord-profile-"));
  for (const d of ["data", "config", "cache"]) mkdirSync(join(profileDir, d));
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
 * Starts tauri-driver. The app inherits its environment from the driver, so profile dirs and the
 * audio backend are fixed per driver process.
 */
export async function startDriver(): Promise<void> {
  if (!profileDir) newProfile();
  driver = spawn("tauri-driver", ["--port", String(driverPort)], {
    stdio: ["ignore", "inherit", "inherit"],
    env: {
      ...process.env,
      XDG_DATA_HOME: join(profileDir, "data"),
      XDG_CONFIG_HOME: join(profileDir, "config"),
      XDG_CACHE_HOME: join(profileDir, "cache"),
      MUMCORD_AUDIO_BACKEND: audio,
    },
  });
  await waitPort(driverPort, true);
}

export async function stopDriver(): Promise<void> {
  driver?.kill("SIGTERM");
  driver = undefined;
  await waitPort(driverPort, false);
  await waitPort(driverPort + 1, false);
}

/** Prepares a fresh install for a new spec file (called from wdio.conf.ts before the session). */
export async function prepareFresh(): Promise<void> {
  newProfile();
  audio = "null";
  await clearKeyring();
  await startDriver();
}

/**
 * Quits the app and starts it again ("restart Mumcord"), optionally with a fresh profile or
 * another audio backend. Keeps the profile by default, so persisted state must survive.
 */
export async function restart(opts: LaunchOptions = {}): Promise<void> {
  await browser.deleteSession();
  await stopDriver();
  if (opts.freshProfile) {
    newProfile();
    await clearKeyring();
  }
  if (opts.audio !== undefined) audio = opts.audio;
  await startDriver();
  await browser.reloadSession();
}
