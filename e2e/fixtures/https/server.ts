// HTTPS fixture for the plugin E2E specs (spec 09, F2): three host names on one TLS listener (an
// origin a plugin declares, one it does not and one only an update declares), a redirect route, a WebSocket echo route and a
// STUN listener. The CA and the server certificate are generated per run in a scratch folder and
// never committed; the app trusts the CA through MUMCORD_TEST_CA.
import { createHash } from "node:crypto";
import { createServer, type Server } from "node:https";
import { createSocket, type Socket } from "node:dgram";
import type { TLSSocket } from "node:tls";
import { readFileSync } from "node:fs";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runOk } from "../../helpers/run.ts";

/** Host names of the fixture; e2e/run.sh maps them to 127.0.0.1 in the container. */
export const DECLARED_HOST = "declared.plugins.test";
export const UNDECLARED_HOST = "undeclared.plugins.test";
export const UPDATE_HOST = "update.plugins.test";
/** Fixed, because plugin manifests declare their origins literally. 18080 stays free for the dev localhost of E2; it is below the ephemeral range for the same reason (see websites.e2e.ts). */
export const HTTPS_PORT = 47443;
export const STUN_PORT = 47478;

const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const STUN_MAGIC = 0x2112a442;

export interface HttpsFixture {
  /** `https://declared.plugins.test:<port>`. */
  declaredOrigin: string;
  /** `https://undeclared.plugins.test:<port>`. */
  undeclaredOrigin: string;
  /** `https://update.plugins.test:<port>`, the origin an updated plugin version declares. */
  updateOrigin: string;
  /** PEM file of the local CA and the server certificate, for MUMCORD_TEST_CA. */
  caFile: string;
  /** UDP port of the STUN listener on 127.0.0.1. */
  stunPort: number;
  /**
   * Everything that reached the fixture since the start or the last `clearLog()`, oldest first:
   * TLS connections by the name they asked for (SNI; also failed handshakes, which a client that
   * does not trust the CA still opens), HTTP requests, and STUN binding requests. A frame probe
   * that leaks shows up here even when the page cannot tell (spec 09 §13.1: "leaks" means a
   * connection reached the collector).
   */
  log(): FixtureHit[];
  clearLog(): void;
  stop(): Promise<void>;
}

/** One thing that reached the fixture. */
export interface FixtureHit {
  kind: "tls" | "http" | "stun";
  /** The host name asked for (SNI or Host header); empty when the client sent none. */
  host: string;
  /** The request path for `http`, else empty. */
  path: string;
}

async function makeCertificates(dir: string): Promise<{ key: Buffer; cert: Buffer; ca: string }> {
  const file = (name: string) => join(dir, name);
  await runOk("openssl", [
    ...["req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes"],
    ...["-keyout", file("ca.key"), "-out", file("ca.pem"), "-days", "2"],
    ...["-subj", "/CN=Mumcord plugin E2E CA"],
    ...["-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=keyCertSign"],
  ]);
  await runOk("openssl", [
    ...["req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes"],
    ...["-keyout", file("server.key"), "-out", file("server.csr")],
    ...["-subj", `/CN=${DECLARED_HOST}`],
  ]);
  writeFileSync(
    file("san.ext"),
    `subjectAltName=DNS:${DECLARED_HOST},DNS:${UNDECLARED_HOST},DNS:${UPDATE_HOST},DNS:localhost,IP:127.0.0.1\n` +
      "extendedKeyUsage=serverAuth\n",
  );
  await runOk("openssl", [
    ...["x509", "-req", "-in", file("server.csr"), "-CA", file("ca.pem"), "-CAkey", file("ca.key")],
    ...["-CAcreateserial", "-out", file("server.pem"), "-days", "2", "-extfile", file("san.ext")],
  ]);
  // MUMCORD_TEST_CA gets the CA and the server certificate: Rust trusts the CA, while WebKitGTK
  // can only accept the exact server certificate for the names it carries.
  const cert = readFileSync(file("server.pem"));
  writeFileSync(file("trust.pem"), Buffer.concat([readFileSync(file("ca.pem")), cert]));
  return { key: readFileSync(file("server.key")), cert, ca: file("trust.pem") };
}

/** The SNI name of a TLS connection; `servername` is `false` when the client sent none. */
function sniName(socket: TLSSocket): string {
  const name = (socket as TLSSocket & { servername?: string | false }).servername;
  return typeof name === "string" ? name : "";
}

/** Echoes unmasked text frames back; enough for a plugin to see a working socket. */
function echoWebSocket(server: Server): void {
  server.on("upgrade", (request, socket) => {
    const key = request.headers["sec-websocket-key"];
    if (request.url !== "/ws" || typeof key !== "string") {
      socket.destroy();
      return;
    }
    const accept = createHash("sha1")
      .update(key + WS_GUID)
      .digest("base64");
    socket.write(
      "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    socket.on("data", (frame: Buffer) => {
      const length = frame.readUInt8(1) & 0x7f;
      if (frame.length < 6 || length > 125) return;
      const mask = frame.subarray(2, 6);
      const payload = Buffer.from(
        frame.subarray(6, 6 + length).map((b, i) => b ^ mask.readUInt8(i % 4)),
      );
      socket.write(Buffer.concat([Buffer.from([0x81, payload.length]), payload]));
    });
    socket.on("error", () => undefined);
  });
}

/** Answers every STUN binding request with 127.0.0.1:<client port> as XOR-MAPPED-ADDRESS. */
function startStun(onRequest: () => void): Promise<Socket> {
  const socket = createSocket("udp4");
  socket.on("message", (message, remote) => {
    if (message.length < 20 || message.readUInt16BE(0) !== 0x0001) return;
    onRequest();
    const attribute = Buffer.alloc(12);
    attribute.writeUInt16BE(0x0020, 0);
    attribute.writeUInt16BE(8, 2);
    attribute.writeUInt8(0x01, 5);
    attribute.writeUInt16BE((remote.port ^ (STUN_MAGIC >>> 16)) & 0xffff, 6);
    const address = remote.address.split(".").reduce((n, part) => n * 256 + Number(part), 0);
    attribute.writeUInt32BE((address ^ STUN_MAGIC) >>> 0, 8);
    const header = Buffer.alloc(20);
    header.writeUInt16BE(0x0101, 0);
    header.writeUInt16BE(attribute.length, 2);
    message.copy(header, 4, 4, 20);
    socket.send(Buffer.concat([header, attribute]), remote.port, remote.address);
  });
  return new Promise((resolvePromise, reject) => {
    socket.once("error", reject);
    socket.bind(STUN_PORT, "127.0.0.1", () => {
      resolvePromise(socket);
    });
  });
}

/**
 * Starts the fixture on its fixed ports. Routes (all hosts): `/` answers "ok <host>", `/redirect`
 * redirects between the declared and the undeclared host, `/ws` is a WebSocket echo.
 */
export async function startHttpsFixture(): Promise<HttpsFixture> {
  const dir = mkdtempSync(join(tmpdir(), "mumcord-https-"));
  const { key, cert, ca } = await makeCertificates(dir);
  const hits: FixtureHit[] = [];
  const server = createServer({ key, cert }, (request, response) => {
    const host = (request.headers.host ?? "").split(":")[0] ?? "";
    hits.push({ kind: "http", host, path: request.url ?? "" });
    if (request.url === "/redirect") {
      const other = host === DECLARED_HOST ? UNDECLARED_HOST : DECLARED_HOST;
      response.writeHead(302, { location: `https://${other}:${String(HTTPS_PORT)}/` });
      response.end();
      return;
    }
    // Like a public API: plugin frames have an opaque origin, so a fetch needs CORS to read the
    // answer. Undeclared hosts never get this far from a frame (its CSP blocks them first).
    response.writeHead(200, { "content-type": "text/plain", "access-control-allow-origin": "*" });
    response.end(`ok ${host}`);
  });
  server.on("secureConnection", (socket: TLSSocket) => {
    hits.push({ kind: "tls", host: sniName(socket), path: "" });
  });
  server.on("tlsClientError", (_error, socket: TLSSocket) => {
    hits.push({ kind: "tls", host: sniName(socket), path: "" });
  });
  echoWebSocket(server);
  await new Promise<void>((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(HTTPS_PORT, "127.0.0.1", resolvePromise);
  });
  const stun = await startStun(() => hits.push({ kind: "stun", host: "", path: "" }));
  return {
    declaredOrigin: `https://${DECLARED_HOST}:${String(HTTPS_PORT)}`,
    undeclaredOrigin: `https://${UNDECLARED_HOST}:${String(HTTPS_PORT)}`,
    updateOrigin: `https://${UPDATE_HOST}:${String(HTTPS_PORT)}`,
    caFile: ca,
    stunPort: stun.address().port,
    log: () => [...hits],
    clearLog() {
      hits.length = 0;
    },
    stop() {
      stun.close();
      server.closeAllConnections();
      return new Promise((resolvePromise) => {
        server.close(() => {
          rmSync(dir, { recursive: true, force: true });
          resolvePromise();
        });
      });
    },
  };
}
