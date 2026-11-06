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
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { startHttpsFixture, type FixtureHit, type HttpsFixture } from "../fixtures/https/server.ts";
import { repoRoot } from "./paths.ts";
import { runOk } from "./run.ts";

const START_TIMEOUT_MS = 30_000;

/** One repository to serve. */
export interface RepositoryOptions {
  /**
   * Folder with the `.mcplugin` packages. Default: the fixture plugins built by
   * scripts/e2e-plugin-registry.sh (`$MUMCORD_E2E_PLUGIN_PACKAGES/packages/...`), else empty.
   */
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
  /** Public key of the third-party repository (it also signs the packages as their author). */
  thirdPartyKey?: string;
  httpsOrigins?: {
    declared: string;
    undeclared: string;
    update: string;
    stunPort: number;
    caFile: string;
  };
}

interface Server {
  name: string;
  child: ChildProcess;
  base: string;
  key: string;
  /** The folder the server lists packages from. */
  packages: string;
  /** Every request the server logged (`<METHOD> <path?query> <status>[ body=<bytes>]`), oldest first. */
  requests: string[];
}

/** The repositories `publish` and `requests` take. */
export type Repository = "official" | "third-party";

let state:
  | {
      servers: Server[];
      https?: HttpsFixture;
      dir: string;
      officialKey?: string;
    }
  | undefined;
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

/** Packages the registry script built for the "official" or "repo" folder, if it was run. */
function builtPackages(folder: string): string | undefined {
  const root = process.env.MUMCORD_E2E_PLUGIN_PACKAGES;
  return root === undefined || root === "" ? undefined : join(root, "packages", folder);
}

/**
 * A package the registry script built (`<id>-<version>.mcplugin`), also those no repository serves
 * from the start (newer versions for the update specs). Throws when the script was not run.
 */
export function stagedPackage(file: string): string {
  const root = process.env.MUMCORD_E2E_PLUGIN_PACKAGES;
  const path = root === undefined || root === "" ? undefined : join(root, "staged", file);
  if (path === undefined || !existsSync(path)) throw new Error(`no built package ${file}`);
  return path;
}

function startServer(
  name: string,
  cli: string,
  args: string[],
  marker: RegExp,
  packages: string,
): Promise<Server> {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cli, ["serve-registry", ...args, "--port", "0"], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    let err = "";
    const requests: string[] = [];
    let partial = "";
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`serve-registry did not start in time\n${out}${err}`));
    }, START_TIMEOUT_MS);
    child.stderr.on("data", (d: Buffer) => {
      err += d.toString();
      // The server logs one line per request; keep them for `requests()` (AC-9.38).
      const lines = (partial + d.toString()).split("\n");
      partial = lines.pop() ?? "";
      for (const line of lines)
        if (/^[A-Z]+ \/\S* \d{3}( body=\d+)?$/.test(line)) requests.push(line);
    });
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
      const base = /(?:base|address) (http:\/\/127\.0\.0\.1:\d+\/)/.exec(out)?.[1];
      const key = marker.exec(out)?.[1];
      if (base !== undefined && key !== undefined) {
        clearTimeout(timer);
        resolvePromise({ name, child, base, key, packages, requests });
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
    const launch = async (
      name: string,
      repo: RepositoryOptions,
      prebuiltFolder: string,
      repository?: string,
    ) => {
      const packages = join(dir, `packages-${name}`);
      mkdirSync(packages, { recursive: true });
      // A copy, so specs that publish into the repository never touch the shared build output.
      const source = repo.packages ?? builtPackages(prebuiltFolder);
      if (source !== undefined && existsSync(source)) cpSync(source, packages, { recursive: true });
      await runOk(cli, ["keygen", "--unencrypted", "--out", dir, "--name", name]);
      const args = ["--packages", packages, "--key", join(dir, `${name}.key`)];
      if (repository !== undefined) args.push("--repository", repository);
      for (const scenario of repo.scenarios ?? []) args.push("--scenario", scenario);
      const server = await startServer(
        name,
        cli,
        args,
        repository === undefined ? /MUMCORD_PLUGIN_REGISTRY_KEY=(\S+)/ : /repository key (\S+)/,
        packages,
      );
      servers.push(server);
      return server;
    };
    if (options.official !== undefined) {
      const server = await launch("official", options.official, "official");
      state.officialKey = server.key;
      next.officialUrl = `${server.base}v1/`;
    }
    if (options.thirdParty !== undefined) {
      const server = await launch("third-party", options.thirdParty, "repo", "E2E third-party");
      next.thirdPartyUrl = `${server.base}v1/index.json`;
      next.thirdPartyKey = server.key;
    }
    if (options.https === true) {
      state.https = await startHttpsFixture();
      next.httpsOrigins = {
        declared: state.https.declaredOrigin,
        undeclared: state.https.undeclaredOrigin,
        update: state.https.updateOrigin,
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

function server(repo: Repository): Server {
  const found = state?.servers.find((s) => s.name === repo);
  if (found === undefined) throw new Error(`the ${repo} repository is not running`);
  return found;
}

/**
 * Copies a package into a running repository and publishes it (the server rescans its folder and
 * raises the index sequence), e.g. a newer version for the update specs.
 */
export async function publish(packageFile: string, repo: Repository): Promise<void> {
  const target = server(repo);
  copyFileSync(packageFile, join(target.packages, basename(packageFile)));
  const response = await fetch(`${target.base}_ctl/reload`, { method: "POST" });
  if (!response.ok) throw new Error(`publishing ${packageFile} failed: ${String(response.status)}`);
}

/** Every request a repository received so far (`<METHOD> <path?query> <status>[ body=<bytes>]`), oldest first. */
export function requests(repo: Repository): string[] {
  return [...server(repo).requests];
}

/**
 * Replaces the active failure scenarios of a running repository (`serve-registry --help`), e.g.
 * `setScenarios("third-party", [])` to serve its `v1.1` index again after `no-v1.1`.
 */
export async function setScenarios(repo: Repository, scenarios: string[]): Promise<void> {
  const target = server(repo);
  // serve-registry reads the query raw (no percent-decoding); scenario names are URL-safe.
  const response = await fetch(`${target.base}_ctl/scenario?set=${scenarios.join(",")}`, {
    method: "POST",
  });
  if (!response.ok) throw new Error(`scenario switch failed: HTTP ${String(response.status)}`);
}

/** What reached the HTTPS fixture (and its STUN listener) since the start or `clearHttpsLog()`. */
export function httpsLog(): FixtureHit[] {
  if (state?.https === undefined) throw new Error("the HTTPS fixture is not running");
  return state.https.log();
}

export function clearHttpsLog(): void {
  state?.https?.clearLog();
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
