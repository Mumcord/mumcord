// The mumpeer test environment (relay + Mumble server + relay bot) for the file sharing, screen
// share and relay-use specs, and the debug-only test hooks of the app (spec 07, B5).
//
// The environment lives in the mumpeer repo (`testenv/`). Check it out next to this repo or point
// `MUMPEER_DIR` at it; e2e/run.sh mounts that folder into the container. Specs that need it call
// `relayAvailable()` and skip themselves otherwise. CI checks mumpeer out only for the shards that
// run such specs (scripts/e2e-shards.js groups them).
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { repoRoot } from "./paths.ts";
import { run, runOk } from "./run.ts";

/** The relay configurations of mumpeer's testenv (`MUMPEER_PROFILE`). */
export type RelayProfile = "all" | "no-files" | "no-screen" | "no-images" | "no-broadcast" | "old";

/** Where a started relay environment listens. */
export interface Relay {
  profile: RelayProfile;
  /** Host the relay announces to clients (`MUMPEER_PUBLIC_HOST`, default 127.0.0.1). */
  host: string;
  /** The Mumble server of the environment (TCP+UDP). */
  port: number;
  /** The relay itself (UDP+TCP). */
  relayPort: number;
  /** The relay's HTTP API (loopback only). */
  apiPort: number;
}

/** The mumpeer checkout: `MUMPEER_DIR`, else a `mumpeer` folder next to this repo. */
export const mumpeerDir = resolve(process.env.MUMPEER_DIR ?? join(repoRoot, "..", "mumpeer"));

const testenvScript = (name: string): string => join(mumpeerDir, "testenv", name);

/** Whether a mumpeer testenv is available; specs that need a relay skip themselves if not. */
export function relayAvailable(): boolean {
  return existsSync(testenvScript("up.sh"));
}

/**
 * Parallel instances (QA agents): `MUMPEER_PORT_BASE` shifts the ports (Mumble server base,
 * relay base+1, API base+2) and `MUMPEER_PROJECT` names the compose project; testenv/up.sh,
 * down.sh and this module read the same variables.
 */
const portBase = Number(process.env.MUMPEER_PORT_BASE ?? 44758);

let current: Relay | undefined;

/** The relay started by `startRelay`; throws when none is running. */
export function relay(): Relay {
  if (!current) throw new Error("no relay environment is running (call startRelay first)");
  return current;
}

export interface StartRelayOptions {
  /** Run an existing mumpeer image instead of building the checkout (profile `old`). */
  image?: string;
  /** The relay bot image (default: the testenv's own, `ghcr.io/mumcord/mumbot:latest`). */
  botImage?: string;
}

const project = process.env.MUMPEER_PROJECT ?? "mumpeer-testenv";
// testenv/env.sh: the default project keeps its data in data/, others in data/_<project>.
const dataRoot =
  project === "mumpeer-testenv"
    ? testenvScript("data")
    : join(testenvScript("data"), `_${project}`);

/**
 * What this module knows about the environment across spec files (each spec file runs in its own
 * worker process): `config` is the running configuration (unset while stopped), `dirty` that a
 * spec registered users on its server since the last data reset.
 */
interface RelayState {
  config?: string;
  dirty: boolean;
}

const stateFile = join(dataRoot, "e2e-state.json");

function readState(): RelayState {
  if (existsSync(stateFile)) return JSON.parse(readFileSync(stateFile, "utf8")) as RelayState;
  // No state file but server data: left behind by an older run, so it may hold registrations.
  return { dirty: existsSync(join(dataRoot, "mumble")) };
}

function writeState(state: RelayState): void {
  mkdirSync(dataRoot, { recursive: true });
  writeFileSync(stateFile, JSON.stringify(state));
}

/**
 * Whether the running environment can serve the next spec file as is: all three services are up
 * (a spec may stop the relay bot) and the relay carries no session of a previous spec file.
 */
async function reusable(): Promise<boolean> {
  const running = await run("docker", [
    "ps",
    "-q",
    "--filter",
    `label=com.docker.compose.project=${project}`,
    "--filter",
    "status=running",
  ]);
  if (running.code !== 0 || running.stdout.trim().split("\n").length !== 3) return false;
  try {
    return (await relayStatus()).sessions === 0;
  } catch {
    return false;
  }
}

/**
 * Starts the mumpeer test environment with a profile, or switches the running one to it, and
 * publishes where it listens as `MUMCORD_E2E_RELAY_HOST` / `MUMCORD_E2E_RELAY_PORT` in this
 * process. The environment is shared by the spec files of a run (wdio.conf.ts stops it at the
 * end): an idle one with the same configuration is reused as is, and one whose server got
 * registrations is wiped first, so every spec file starts on an empty server without anyone else's
 * relay sessions. Without `MUMPEER_IMAGE` the first call builds the mumpeer image (minutes).
 */
export async function startRelay(
  profile: RelayProfile = "all",
  opts: StartRelayOptions = {},
): Promise<Relay> {
  if (!relayAvailable()) {
    throw new Error(`no mumpeer testenv at ${mumpeerDir} (set MUMPEER_DIR to a mumpeer checkout)`);
  }
  const config = JSON.stringify({ profile, image: opts.image, botImage: opts.botImage });
  current = {
    profile,
    host: process.env.MUMPEER_PUBLIC_HOST ?? "127.0.0.1",
    port: portBase,
    relayPort: portBase + 1,
    apiPort: portBase + 2,
  };
  const state = readState();
  if (state.dirty) {
    await resetRelayData();
  } else if (state.config === config && (await reusable())) {
    console.log(`[relayenv] reusing the running '${profile}' environment`);
    publish(current);
    return current;
  }
  const env: NodeJS.ProcessEnv = { MUMPEER_PROFILE: profile };
  if (opts.image) env.MUMPEER_IMAGE = opts.image;
  if (opts.botImage) env.MUMBOT_IMAGE = opts.botImage;
  // up.sh recreates the relay and the server with the new profile; no stop needed in between.
  await runOk(testenvScript("up.sh"), [], env);
  writeState({ config, dirty: false });
  publish(current);
  return current;
}

function publish(r: Relay): void {
  process.env.MUMCORD_E2E_RELAY_HOST = r.host;
  process.env.MUMCORD_E2E_RELAY_PORT = String(r.port);
}

/**
 * Notes that a user registers on the environment's server: the relay bot hands out tokens to
 * registered users only, and a registered name would lock later spec files (with new
 * certificates) out, so the next `startRelay` wipes the server data first.
 */
export function noteRegistration(): void {
  if (relayAvailable()) writeState({ ...readState(), dirty: true });
}

/**
 * Stops the environment `startRelay` started (no-op otherwise); data, secret and relay key are
 * kept.
 */
export async function stopRelay(): Promise<void> {
  if (!relayAvailable() || readState().config === undefined) return;
  await runOk(testenvScript("down.sh"), []);
  writeState({ dirty: readState().dirty });
  current = undefined;
}

/**
 * Stops the environment and deletes its data: the server database with every registration, the
 * relay key and the relay bot's identity.
 */
async function resetRelayData(): Promise<void> {
  await runOk(testenvScript("down.sh"), []);
  if (project !== "mumpeer-testenv") {
    rmSync(dataRoot, { recursive: true, force: true });
    return;
  }
  // The default project's data sits next to the folders of parallel runs (`_<project>`).
  if (!existsSync(dataRoot)) return;
  for (const entry of readdirSync(dataRoot)) {
    if (!entry.startsWith("_")) rmSync(join(dataRoot, entry), { recursive: true, force: true });
  }
}

/** The relay's `/v1/status`, parsed (the `uses` list and `broadcast` limits show the profile). */
export async function relayStatus(): Promise<{
  uses?: string[];
  broadcast?: { max_viewers: number };
  [key: string]: unknown;
}> {
  const { apiPort } = relay();
  const envFile = readFileSync(testenvScript(".env"), "utf8");
  const secret = /^MUMPEER_API_SECRET=(.+)$/m.exec(envFile)?.[1];
  if (!secret) throw new Error("MUMPEER_API_SECRET missing in the testenv .env");
  const res = await fetch(`http://127.0.0.1:${String(apiPort)}/v1/status`, {
    headers: { Authorization: `Bearer ${secret}` },
  });
  if (!res.ok) throw new Error(`relay /v1/status answered ${String(res.status)}`);
  return (await res.json()) as Awaited<ReturnType<typeof relayStatus>>;
}

/**
 * Debug-only test hooks of the app (spec 07), turned into app environment by `hookEnv`. Release
 * builds ignore them.
 */
export interface TestHooks {
  /** Share lifetime in seconds instead of 24 h (AC-7.15). */
  fileExpirySeconds?: number;
  /** Flip one byte of a file in transit (AC-7.11). */
  fileCorrupt?: boolean;
  /** Stop sending file bytes after this many bytes; the client stays connected (F3a (e), issue #420). */
  fileStallAfterBytes?: number;
  /** Force encoder probe results, e.g. `["av1_hw:smoke_fail"]` (AC-7.36, 7.37). */
  encoderProbe?: string[];
  /** Force the SVT-AV1 self-test result (AC-7.37). */
  svtSelftest?: "pass" | "fail";
  /** Delay every encoded frame by this many ms (AC-7.38). */
  encodeDelayMs?: number;
  /** Pretend the machine runs on battery (AC-7.40). */
  onBattery?: boolean;
  /** Pretend WebCodecs cannot decode AV1 (AC-7.41). */
  noWebcodecsAv1?: boolean;
  /** Keep the previous broadcast key after a rotation (AC-7.30). */
  keepOldKey?: boolean;
}

/**
 * Environment for `app.restart`/`apps.startB`/`startC`/`restartB`/`restartC` (`env` option) that
 * switches on the test hooks of spec 07. Every spec file starts without them.
 */
export function hookEnv(h: TestHooks): Record<string, string> {
  const env: Record<string, string> = {};
  if (h.fileExpirySeconds !== undefined)
    env.MUMCORD_TEST_FILE_EXPIRY_S = String(h.fileExpirySeconds);
  if (h.fileCorrupt) env.MUMCORD_TEST_FILE_CORRUPT = "1";
  if (h.fileStallAfterBytes !== undefined)
    env.MUMCORD_TEST_FILE_STALL_AFTER = String(h.fileStallAfterBytes);
  if (h.encoderProbe !== undefined) env.MUMCORD_TEST_ENCODER_PROBE = h.encoderProbe.join(",");
  if (h.svtSelftest !== undefined) env.MUMCORD_TEST_SVT_SELFTEST = h.svtSelftest;
  if (h.encodeDelayMs !== undefined) env.MUMCORD_TEST_ENCODE_DELAY_MS = String(h.encodeDelayMs);
  if (h.onBattery) env.MUMCORD_TEST_ON_BATTERY = "1";
  if (h.noWebcodecsAv1) env.MUMCORD_TEST_NO_WEBCODECS_AV1 = "1";
  if (h.keepOldKey) env.MUMCORD_TEST_KEEP_OLD_KEY = "1";
  return env;
}
