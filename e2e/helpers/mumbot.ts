// The local mumbot of testenv/mumbot (music module with its HTTP API) for the Mumbot Music specs.
// Each start uses a fresh random API key; it lives only in the git-ignored rendered config.
import { randomBytes } from "node:crypto";
import { createServer, type Server } from "node:http";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { bin, testenvDir } from "./paths.ts";
import { runOk } from "./run.ts";
import { ports } from "./testenv.ts";

/** Channel ids written by testenv/up.sh (Root is 0). */
export const channelIds = { Lobby: 1, Raid: 2, AFK: 3 } as const;

export interface LocalMumbot {
  /** `127.0.0.1:<port>` of the music API, as a user types it into the plugin. */
  address: string;
  /** The API key the bot accepts. */
  key: string;
  /** Calls the music API with the key; throws on a non-2xx answer. */
  api<T>(method: "GET" | "POST", path: string, body?: object): Promise<T>;
  stop(): Promise<void>;
}

/** Starts the bot (needs v15 up) and waits until it is connected to the server. */
export async function start(): Promise<LocalMumbot> {
  const key = randomBytes(16).toString("hex");
  // Next to the test servers' ports so parallel runs (MUMCORD_PORT_BASE) never share it.
  const port = ports.v15 + 60;
  const env = { MUMBOT_API_KEY: key, MUMBOT_PORT: String(port), MUMBOT_BIN: bin.bot };
  await runOk(join(testenvDir, "mumbot", "up.sh"), [], env);
  const base = `http://127.0.0.1:${String(port)}`;
  const deadline = Date.now() + 60_000;
  for (;;) {
    const health = await fetch(`${base}/health`)
      .then((r) => r.json() as Promise<{ connected?: boolean }>)
      .catch(() => null);
    if (health?.connected === true) break;
    if (Date.now() > deadline) throw new Error("the local mumbot did not connect to v15 in 60 s");
    await new Promise((r) => setTimeout(r, 1000));
  }
  return {
    address: `127.0.0.1:${String(port)}`,
    key,
    async api<T>(method: "GET" | "POST", path: string, body?: object): Promise<T> {
      // The plugin under test polls with the same key (every 3 s of the bot's 30 requests per
      // minute), so the rate limit (429) can hit the test too: wait and retry.
      for (let attempt = 1; ; attempt++) {
        const response = await fetch(`${base}${path}`, {
          method,
          headers: { "X-API-Key": key, "Content-Type": "application/json" },
          body: body === undefined ? undefined : JSON.stringify(body),
        });
        if (response.status === 429 && attempt < 10) {
          await new Promise((r) => setTimeout(r, 3000));
          continue;
        }
        if (!response.ok) throw new Error(`mumbot ${method} ${path}: ${String(response.status)}`);
        return (await response.json()) as T;
      }
    },
    async stop() {
      await runOk(join(testenvDir, "mumbot", "down.sh"), []);
    },
  };
}

export interface TrackServer {
  /** URL of the served MP3 as the bot (in its container) reaches it. */
  url: string;
  close(): Promise<void>;
}

/**
 * Serves one MP3 over plain HTTP on all host addresses, so the bot's container reaches it as
 * `host.docker.internal` (the only URL host its config allows).
 */
export async function serveTrack(file: string, name: string): Promise<TrackServer> {
  const data = await readFile(file);
  const server: Server = createServer((req, res) => {
    if (req.url !== `/${name}`) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "Content-Type": "audio/mpeg", "Content-Length": data.length }).end(data);
  });
  await new Promise<void>((resolve) => server.listen(0, "0.0.0.0", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("no port for the server");
  return {
    url: `http://host.docker.internal:${String(address.port)}/${name}`,
    close: () =>
      new Promise((resolve) =>
        server.close(() => {
          resolve();
        }),
      ),
  };
}

/** The test MP3s of testenv/mumbot/media (6 s sine tones). */
export const media = (name: "tone-a.mp3" | "tone-b.mp3"): string =>
  join(testenvDir, "mumbot", "media", name);
