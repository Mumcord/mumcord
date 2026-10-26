import { join } from "node:path";
import { testenvDir } from "./paths.ts";
import { runOk } from "./run.ts";

export type Service = "v15" | "v14" | "v13" | "limit" | "v15b";
export type CertSet = "A" | "B";

/**
 * Parallel instances (QA agents): `MUMCORD_PORT_BASE` shifts all server ports (default 64738 = v15,
 * then v14, v13, limit, v15b), `MUMCORD_PROJECT` names the compose project (default `testenv`).
 * The same variables are read by testenv/up.sh, down.sh and reset.sh.
 */
const portBase = Number(process.env.MUMCORD_PORT_BASE ?? 64738);
export const project = process.env.MUMCORD_PROJECT ?? "testenv";

/** Host ports of the test servers (TCP+UDP); the container shares the host network. */
export const ports: Record<Service, number> = {
  v15: portBase,
  v14: portBase + 1,
  v13: portBase + 2,
  limit: portBase + 3,
  v15b: portBase + 4,
};

/** `host:port` of a test server, as typed into the client's add-server form. */
export const address = (svc: Service): string => `localhost:${String(ports[svc])}`;

const container = (svc: Service): string => `${project}-${svc}-1`;

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
  await runOk("docker", ["stop", container(svc)]);
}

export async function start(svc: Service): Promise<void> {
  await runOk("docker", ["start", container(svc)]);
}

export async function restart(svc: Service): Promise<void> {
  await runOk("docker", ["restart", container(svc)]);
}
