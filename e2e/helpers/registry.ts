// Fixture plugin registry for E2E (spec 06, sections 10/11).
//
// `start()` runs scripts/e2e-plugin-registry.sh (fixtures, the four official plugins, fresh TEST
// keys) and then serves two registries on free 127.0.0.1 ports with `mumcord-plugin
// serve-registry`: the "official" one and one third-party repository. The keys are generated per
// run in the run's cache folder and never committed. Debug builds accept the registry key through
// `MUMCORD_PLUGIN_REGISTRY_KEY`, which `env()` sets next to the registry URL.
import { type ChildProcess, spawn } from "node:child_process";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { pluginEnv, type PluginTestEnv } from "./apps.ts";
import { repoRoot } from "./paths.ts";
import { runOk } from "./run.ts";

const START_TIMEOUT_MS = 30_000;

/** One running `serve-registry` process. */
interface Server {
  child: ChildProcess;
  /** `http://127.0.0.1:<port>/`. */
  base: string;
  /** Public key line of the server's signing key. */
  key: string;
}

export interface FixtureRegistry {
  /** Base URL of the official registry (`.../v1/`), for `MUMCORD_PLUGIN_REGISTRY_URL`. */
  registryUrl: string;
  /** Public minisign key of the official registry, for `MUMCORD_PLUGIN_REGISTRY_KEY`. */
  registryKey: string;
  /** Address of the third-party repository's `index.json`, to add under Repositories. */
  repoUrl: string;
  /** Public key line of the third-party repository. */
  repoKey: string;
  /** Folder with the packages of the run (`staged/` holds every fixture). */
  workDir: string;
  /** Environment for `app.restart` / `apps.startB`; extra test settings can be added. */
  env(extra?: Omit<PluginTestEnv, "registryKey" | "registryUrl">): Record<string, string>;
  /** Sets the active failure scenarios of the official registry (empty clears them). */
  setScenario(...scenarios: string[]): Promise<void>;
  /** Same for the third-party repository. */
  setRepoScenario(...scenarios: string[]): Promise<void>;
  /**
   * Adds a staged fixture package (file name without extension, e.g. `fx-update-v2-fails`) to the
   * official registry (`repo: false`) or the repository (`repo: true`) and publishes it.
   */
  publish(fixture: string, options?: { repo?: boolean }): Promise<void>;
  /** Rescans the package folders and raises the sequence. */
  reload(): Promise<void>;
  /** Stops both servers and deletes the run's keys. */
  stop(): Promise<void>;
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

async function control(server: Server, path: string): Promise<void> {
  const response = await fetch(`${server.base}_ctl/${path}`, { method: "POST" });
  if (!response.ok) throw new Error(`registry control ${path} failed: ${String(response.status)}`);
}

/**
 * Builds the fixtures and starts both registries. Call `stop()` in `after`. A cold build takes
 * minutes, so give the `before` hook a long timeout.
 */
export async function start(workDir?: string): Promise<FixtureRegistry> {
  const dir =
    workDir ??
    process.env.MUMCORD_E2E_REGISTRY_DIR ??
    join(homedir(), ".cache", "mumcord-build", "e2e-plugin-registry", String(process.pid));
  const script = join(repoRoot, "scripts", "e2e-plugin-registry.sh");
  await runOk("bash", [script, dir]);

  const cli = join(dir, "bin", "mumcord-plugin");
  const official = await startServer(
    cli,
    ["--packages", join(dir, "packages", "official"), "--key", join(dir, "keys", "registry.key")],
    /MUMCORD_PLUGIN_REGISTRY_KEY=(\S+)/,
  );
  let repo: Server;
  try {
    repo = await startServer(
      cli,
      [
        "--packages",
        join(dir, "packages", "repo"),
        "--key",
        join(dir, "keys", "repo.key"),
        "--repository",
        "E2E fixtures",
      ],
      /repository key (\S+)/,
    );
  } catch (error) {
    official.child.kill();
    throw error;
  }

  const registryUrl = `${official.base}v1/`;
  const packages = (isRepo: boolean | undefined) =>
    join(dir, "packages", isRepo === true ? "repo" : "official");
  const scenario = (server: Server, scenarios: string[]) =>
    control(server, `scenario?set=${encodeURIComponent(scenarios.join(","))}`);

  return {
    registryUrl,
    registryKey: official.key,
    repoUrl: `${repo.base}v1/index.json`,
    repoKey: repo.key,
    workDir: dir,
    env: (extra) => pluginEnv({ ...extra, registryKey: official.key, registryUrl }),
    setScenario: (...scenarios) => scenario(official, scenarios),
    setRepoScenario: (...scenarios) => scenario(repo, scenarios),
    async publish(fixture, options) {
      mkdirSync(packages(options?.repo), { recursive: true });
      copyFileSync(
        join(dir, "staged", `${fixture}.mcplugin`),
        join(packages(options?.repo), `${fixture}.mcplugin`),
      );
      await control(options?.repo === true ? repo : official, "reload");
    },
    async reload() {
      await control(official, "reload");
      await control(repo, "reload");
    },
    stop() {
      official.child.kill();
      repo.child.kill();
      rmSync(join(dir, "keys"), { recursive: true, force: true });
      return Promise.resolve();
    },
  };
}
