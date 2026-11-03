// HTTPS fixture for the plugin E2E specs (spec 09, F2): three host names on one TLS listener (an
// origin a plugin declares, one it does not and one only an update declares), a redirect route, a WebSocket echo route and a
// STUN listener. The CA and the server certificate are generated per run in a scratch folder and
// never committed; the app trusts the CA through MUMCORD_TEST_CA.
import { createHash } from "node:crypto";
import { createServer, type Server } from "node:https";
import { createSocket, type Socket } from "node:dgram";
import { readFileSync } from "node:fs";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runOk } from "../../helpers/run.ts";

/** Host names of the fixture; e2e/run.sh maps them to 127.0.0.1 in the container. */
export const DECLARED_HOST = "declared.plugins.test";
export const UNDECLARED_HOST = "undeclared.plugins.test";
export const UPDATE_HOST = "update.plugins.test";
/** Fixed, because plugin manifests declare their origins literally. 47180 stays free for the dev localhost of E2. */
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
  /** PEM file of the local CA, for MUMCORD_TEST_CA. */
  caFile: string;
  /** UDP port of the STUN listener on 127.0.0.1. */
  stunPort: number;
  stop(): Promise<void>;
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
  return {
    key: readFileSync(file("server.key")),
    cert: readFileSync(file("server.pem")),
    ca: file("ca.pem"),
  };
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
function startStun(): Promise<Socket> {
  const socket = createSocket("udp4");
  socket.on("message", (message, remote) => {
    if (message.length < 20 || message.readUInt16BE(0) !== 0x0001) return;
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
  const server = createServer({ key, cert }, (request, response) => {
    const host = (request.headers.host ?? "").split(":")[0] ?? "";
    if (request.url === "/redirect") {
      const other = host === DECLARED_HOST ? UNDECLARED_HOST : DECLARED_HOST;
      response.writeHead(302, { location: `https://${other}:${String(HTTPS_PORT)}/` });
      response.end();
      return;
    }
    response.writeHead(200, { "content-type": "text/plain" });
    response.end(`ok ${host}`);
  });
  echoWebSocket(server);
  await new Promise<void>((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(HTTPS_PORT, "127.0.0.1", resolvePromise);
  });
  const stun = await startStun();
  return {
    declaredOrigin: `https://${DECLARED_HOST}:${String(HTTPS_PORT)}`,
    undeclaredOrigin: `https://${UNDECLARED_HOST}:${String(HTTPS_PORT)}`,
    updateOrigin: `https://${UPDATE_HOST}:${String(HTTPS_PORT)}`,
    caFile: ca,
    stunPort: stun.address().port,
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
