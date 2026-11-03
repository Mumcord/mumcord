// Fixtures for the plugin platform 1.1 specs (spec 09, F2): an official and a third-party
// repository served by `mumcord-plugin serve-registry` (each with `v1/` and `v1.1/` indexes) and a
// local HTTPS fixture with its own CA. The keys and the CA are generated per run and never
// committed. Plugin packages come from the caller (`packages`); the test plugins are built by F1.
//
//   const fx = await pluginenv.startPluginFixtures({ official: {}, thirdParty: {}, https: true });
//   await app.restart({ env: pluginenv.appEnv() });
//   ...
//   await pluginenv.stopPluginFixtures();
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startHttpsFixture, type HttpsFixture } from "../fixtures/https/server.ts";
import { repoRoot } from "./paths.ts";
import { runOk } from "./run.ts";

const START_TIMEOUT_MS = 30_000;

/** One repository to serve. */
export interface RepositoryOptions {
  /** Folder with the `.mcplugin` packages (default: an empty one). */
  packages?: string;
  /** Failure scenarios active from the start, e.g. `no-v1.1` (see `serve-registry --help`). */
  scenarios?: string[];
}

export interface PluginFixtureOptions {
  /** Serve an official registry (its key is trusted through `appEnv`). */
  official?: RepositoryOptions;
  /** Serve a third-party repository. */
  thirdParty?: RepositoryOptions;
  /** Start the HTTPS fixture with its CA. */
  https?: boolean;
}

export interface PluginFixtures {
  /** Official registry `v1/` base (for MUMCORD_PLUGIN_REGISTRY_URL); `v1.1/` is its sibling. */
  officialUrl?: string;
  /** Third-party `v1/index.json`; the `v1.1/index.json` sibling is next to it. */
  thirdPartyUrl?: string;
  httpsOrigins?: { declared: string; undeclared: string; stunPort: number; caFile: string };
}

interface Server {
  child: ChildProcess;
  base: string;
  key: string;
}

let state:
  { servers: Server[]; https?: HttpsFixture; dir: string; officialKey?: string } | undefined;
let current: PluginFixtures = {};

/** The CLI: `MUMCORD_PLUGIN_CLI`, else the debug build (built on first use). */
async function cliPath(): Promise<string> {
  const given = process.env.MUMCORD_PLUGIN_CLI;
  if (given !== undefined && given !== "") return given;
  const target = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target");
  const path = join(target, "debug", "mumcord-plugin");
  if (!existsSync(path)) {
    await runOk("cargo", ["build", "--locked", "--quiet", "-p", "mumcord-plugin"]);
  }
  return path;
}

function startServer(cli: string, args: string[], marker: RegExp): Promise<Server> {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cli, ["serve-registry", ...args, "--port", "0"], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    let err = "";
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`serve-registry did not start in time\n${out}${err}`));
    }, START_TIMEOUT_MS);
    child.stderr.on("data", (d: Buffer) => (err += d.toString()));
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
      const base = /(?:base|address) (http:\/\/127\.0\.0\.1:\d+\/)/.exec(out)?.[1];
      const key = marker.exec(out)?.[1];
      if (base !== undefined && key !== undefined) {
        clearTimeout(timer);
        resolvePromise({ child, base, key });
      }
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`serve-registry exited early (${String(code)})\n${out}${err}`));
    });
  });
}

/** Starts the requested fixtures; call `stopPluginFixtures()` in `after`. */
export async function startPluginFixtures(options: PluginFixtureOptions): Promise<PluginFixtures> {
  if (state !== undefined) await stopPluginFixtures();
  const dir = mkdtempSync(join(tmpdir(), "mumcord-pluginenv-"));
  const servers: Server[] = [];
  const next: PluginFixtures = {};
  state = { servers, dir };
  try {
    const cli = await cliPath();
    const launch = async (name: string, repo: RepositoryOptions, repository?: string) => {
      const packages = repo.packages ?? join(dir, `packages-${name}`);
      mkdirSync(packages, { recursive: true });
      await runOk(cli, ["keygen", "--unencrypted", "--out", dir, "--name", name]);
      const args = ["--packages", packages, "--key", join(dir, `${name}.key`)];
      if (repository !== undefined) args.push("--repository", repository);
      for (const scenario of repo.scenarios ?? []) args.push("--scenario", scenario);
      const server = await startServer(
        cli,
        args,
        repository === undefined ? /MUMCORD_PLUGIN_REGISTRY_KEY=(\S+)/ : /repository key (\S+)/,
      );
      servers.push(server);
      return server;
    };
    if (options.official !== undefined) {
      const server = await launch("official", options.official);
      state.officialKey = server.key;
      next.officialUrl = `${server.base}v1/`;
    }
    if (options.thirdParty !== undefined) {
      const server = await launch("third-party", options.thirdParty, "E2E third-party");
      next.thirdPartyUrl = `${server.base}v1/index.json`;
    }
    if (options.https === true) {
      state.https = await startHttpsFixture();
      next.httpsOrigins = {
        declared: state.https.declaredOrigin,
        undeclared: state.https.undeclaredOrigin,
        stunPort: state.https.stunPort,
        caFile: state.https.caFile,
      };
    }
  } catch (error) {
    await stopPluginFixtures();
    throw error;
  }
  current = next;
  return next;
}

/** Stops every fixture and deletes the run's keys and CA. Safe to call twice. */
export async function stopPluginFixtures(): Promise<void> {
  const old = state;
  state = undefined;
  current = {};
  if (old === undefined) return;
  for (const server of old.servers) server.child.kill();
  await old.https?.stop();
  rmSync(old.dir, { recursive: true, force: true });
}

/** Test hooks of the running fixtures (registry URL and key, CA) plus `extra` (e.g. KEY_AGE). */
export function appEnv(extra: Record<string, string> = {}): Record<string, string> {
  const env: Record<string, string> = {};
  if (current.officialUrl !== undefined && state?.officialKey !== undefined) {
    env.MUMCORD_PLUGIN_REGISTRY_URL = current.officialUrl;
    env.MUMCORD_PLUGIN_REGISTRY_KEY = state.officialKey;
  }
  if (current.httpsOrigins !== undefined) {
    env.MUMCORD_TEST_CA = current.httpsOrigins.caFile;
    env.MUMCORD_TEST_PUBLIC_ADDR = "127.0.0.1";
  }
  return { ...env, ...extra };
}
