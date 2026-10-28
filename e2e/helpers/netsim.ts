import { spawn, type ChildProcess } from "node:child_process";
import { bin } from "./paths.ts";
import { runOk } from "./run.ts";
import { shared } from "./state.ts";
import { ports, type Service } from "./testenv.ts";

/**
 * Fixed ports: netsim's defaults collide with testenv v14/v13. Point the client at `listenPort`.
 * They sit 62/63 above `MUMCORD_PORT_BASE` (64800/64801 by default), so parallel instances do not collide.
 */
const base = ports.v15;
export const listenPort = base + 62;
export const controlPort = base + 63;

/** One netsim proxy. `ctl("loss", "both", "10")`, `ctl("block-udp", "on")`, `ctl("kill-tcp")`, `ctl("reset")` … */
export interface Proxy {
  /** Port clients or bots connect to instead of the server port. */
  listenPort: number;
  ctl(...args: string[]): Promise<string>;
  stop(): void;
}

/**
 * Starts a proxy 127.0.0.1:`listen` -> the given test server (TCP+UDP). Use several to put each of
 * multiple servers (or a bot) behind its own proxy: pass distinct `listen`/`control` ports at
 * `listenPort + 2 * n` and `controlPort + 2 * n` (n >= 1; n = 0 is the default proxy below).
 */
export async function startProxy(
  target: Service,
  opts: { listen?: number; control?: number } = {},
): Promise<Proxy> {
  const listen = opts.listen ?? listenPort;
  const controlAddr = `127.0.0.1:${String(opts.control ?? controlPort)}`;
  const child: ChildProcess = spawn(bin.netsim, [
    "run",
    "--listen",
    `127.0.0.1:${String(listen)}`,
    "--target",
    `127.0.0.1:${String(ports[target])}`,
    "--control",
    controlAddr,
  ]);
  const ctlOn = async (...args: string[]): Promise<string> =>
    (await runOk(bin.netsim, ["ctl", "--control", controlAddr, ...args])).stdout;
  for (let i = 0; i < 50; i++) {
    try {
      await ctlOn("status");
      return { listenPort: listen, ctl: ctlOn, stop: () => child.kill("SIGTERM") };
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  child.kill("SIGTERM");
  throw new Error("netsim did not start");
}

// Shared with the wdio config (see state.ts): the specs start the proxy, afterSession stops it.
const state = shared("netsim", { proxy: undefined as Proxy | undefined });

/** Starts the default proxy 127.0.0.1:listenPort -> the given test server (TCP+UDP). */
export async function start(target: Service = "v15"): Promise<void> {
  if (state.proxy) throw new Error("netsim already running");
  state.proxy = await startProxy(target);
}

export async function ctl(...args: string[]): Promise<string> {
  if (state.proxy) return state.proxy.ctl(...args);
  return (
    await runOk(bin.netsim, ["ctl", "--control", `127.0.0.1:${String(controlPort)}`, ...args])
  ).stdout;
}

export function stop(): void {
  state.proxy?.stop();
  state.proxy = undefined;
}
