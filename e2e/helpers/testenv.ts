import { join } from "node:path";
import { testenvDir } from "./paths.ts";
import { runOk } from "./run.ts";

export type Service = "v15" | "v14" | "v13" | "limit";
export type CertSet = "A" | "B";

/** Host ports of the test servers (TCP+UDP); the container shares the host network. */
export const ports: Record<Service, number> = { v15: 64738, v14: 64739, v13: 64740, limit: 64741 };

function certArgs(cert?: CertSet): string[] {
  return cert ? ["--cert", cert] : [];
}

/** Starts (or recreates with another certificate set) a test server. */
export async function up(svc: Service, opts: { cert?: CertSet } = {}): Promise<void> {
  await runOk(join(testenvDir, "up.sh"), [svc, ...certArgs(opts.cert)]);
}

/** Restores the fresh seeded state. Drops connected clients: never while another test uses it. */
export async function reset(svc: Service, opts: { cert?: CertSet } = {}): Promise<void> {
  await runOk(join(testenvDir, "reset.sh"), [svc, ...certArgs(opts.cert)]);
}

/** Stops a server container (reconnect tests); `start` brings it back with the same data. */
export async function stop(svc: Service): Promise<void> {
  await runOk("docker", ["stop", `testenv-${svc}-1`]);
}

export async function start(svc: Service): Promise<void> {
  await runOk("docker", ["start", `testenv-${svc}-1`]);
}

export async function restart(svc: Service): Promise<void> {
  await runOk("docker", ["restart", `testenv-${svc}-1`]);
}
