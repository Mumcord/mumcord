import { spawn, type ChildProcess } from "node:child_process";
import { bin } from "./paths.ts";
import { runOk } from "./run.ts";
import { ports, type Service } from "./testenv.ts";

/** Fixed ports: netsim's defaults collide with testenv v14/v13. Point the client at `listenPort`. */
export const listenPort = 64800;
export const controlPort = 64801;
const control = `127.0.0.1:${String(controlPort)}`;

let proxy: ChildProcess | undefined;

/** Starts the proxy 127.0.0.1:64800 -> the given test server (TCP+UDP). */
export async function start(target: Service = "v15"): Promise<void> {
  if (proxy) throw new Error("netsim already running");
  proxy = spawn(bin.netsim, [
    "run",
    "--listen",
    `127.0.0.1:${String(listenPort)}`,
    "--target",
    `127.0.0.1:${String(ports[target])}`,
    "--control",
    control,
  ]);
  for (let i = 0; i < 50; i++) {
    try {
      await ctl("status");
      return;
    } catch {
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  throw new Error("netsim did not start");
}

/** `ctl("loss", "both", "10")`, `ctl("block-udp", "on")`, `ctl("kill-tcp")`, `ctl("reset")` … */
export async function ctl(...args: string[]): Promise<string> {
  return (await runOk(bin.netsim, ["ctl", "--control", control, ...args])).stdout;
}

export function stop(): void {
  proxy?.kill("SIGTERM");
  proxy = undefined;
}
