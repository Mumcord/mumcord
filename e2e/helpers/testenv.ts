import { join } from "node:path";
import { connect as tlsConnect } from "node:tls";
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

function acceptsTls(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    // Docker can report a running container before Mumble accepts connections. A TCP
    // proxy alone is not sufficient: finish a TLS handshake with the local test server.
    const socket = tlsConnect({
      host: "127.0.0.1",
      port,
      rejectUnauthorized: false,
      timeout: 1000,
    });
    let settled = false;
    const finish = (ready: boolean) => {
      if (settled) return;
      settled = true;
      socket.destroy();
      resolve(ready);
    };
    socket.once("secureConnect", () => {
      finish(true);
    });
    socket.once("error", () => {
      finish(false);
    });
    socket.once("timeout", () => {
      finish(false);
    });
  });
}

async function waitReady(svc: Service): Promise<void> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (await acceptsTls(ports[svc])) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`${svc} did not accept TLS on port ${String(ports[svc])} within 30 seconds`);
}

/** Starts (or recreates with another certificate set) a test server. */
export async function up(svc: Service, opts: { cert?: CertSet } = {}): Promise<void> {
  await runOk(join(testenvDir, "up.sh"), [svc, ...certArgs(opts.cert)]);
  await waitReady(svc);
}

/** Restores the fresh seeded state. Drops connected clients: never while another test uses it. */
export async function reset(svc: Service, opts: { cert?: CertSet } = {}): Promise<void> {
  await runOk(join(testenvDir, "reset.sh"), [svc, ...certArgs(opts.cert)]);
  await waitReady(svc);
}

/** Stops a server container (reconnect tests); `start` brings it back with the same data. */
export async function stop(svc: Service): Promise<void> {
  await runOk("docker", ["stop", container(svc)]);
}

export async function start(svc: Service): Promise<void> {
  await runOk("docker", ["start", container(svc)]);
  await waitReady(svc);
}

export async function restart(svc: Service): Promise<void> {
  await runOk("docker", ["restart", container(svc)]);
  await waitReady(svc);
}
