/**
 * Packet inspector of the mock engine: a fixed set of fictional packets appears when capture
 * starts, so the inspector can be tried and photographed without a server.
 */
import type { MediaPacket, Packet, PacketCapture } from "../bindings/commands";

function hex(text: string): string {
  return Array.from(new TextEncoder().encode(text), (b) => b.toString(16).padStart(2, "0")).join(
    "",
  );
}

function plugin(
  id: number,
  atMs: number,
  direction: Packet["direction"],
  kind: string,
  payload: unknown,
  peers: { sender?: number; receivers?: number[] },
  pacing?: { delayMs: number; tokensLeft: number },
): Packet {
  const text = JSON.stringify(payload);
  return {
    id,
    serverId: 1,
    atMs,
    direction,
    kind,
    pluginData: true,
    size: new TextEncoder().encode(text).length,
    sender: peers.sender ?? null,
    receivers: peers.receivers ?? [],
    delayMs: pacing?.delayMs ?? null,
    tokensLeft: pacing?.tokensLeft ?? null,
    decoded: JSON.stringify(payload, null, 2),
    rawHex: hex(text),
    media: null,
  };
}

function control(
  id: number,
  atMs: number,
  direction: Packet["direction"],
  kind: string,
  size: number,
): Packet {
  return {
    id,
    serverId: 1,
    atMs,
    direction,
    kind,
    pluginData: false,
    size,
    sender: null,
    receivers: [],
    delayMs: null,
    tokensLeft: null,
    decoded: null,
    rawHex: "",
    media: null,
  };
}

/** One message of a media stream: no payload, so no hex, and a description only on channel 0. */
function media(
  id: number,
  atMs: number,
  direction: Packet["direction"],
  size: number,
  message: MediaPacket,
  decoded: string | null = null,
): Packet {
  return {
    id,
    serverId: 1,
    atMs,
    direction,
    kind: message.format === "media" ? ".media" : `.media legacy v${String(message.version)}`,
    pluginData: false,
    size,
    sender: null,
    receivers: [],
    delayMs: null,
    tokensLeft: null,
    decoded,
    rawHex: "",
    media: message,
  };
}

function samplePackets(start: number): Packet[] {
  return [
    plugin(
      0,
      start,
      "sent",
      "mumcord.hello",
      { v: 1, app: "mumcord", features: ["watch", "screen"] },
      { receivers: [2, 3, 4] },
      { delayMs: 0, tokensLeft: 11 },
    ),
    plugin(
      1,
      start + 120,
      "received",
      "mumcord.hello",
      { v: 1, app: "mumcord", features: ["watch"] },
      { sender: 2 },
    ),
    plugin(
      2,
      start + 460,
      "sent",
      "mumcord.watch",
      { v: 1, kind: "state", url: "https://example.com/video", playing: true, position: 42.5 },
      { receivers: [2] },
      { delayMs: 340, tokensLeft: 2 },
    ),
    control(3, start + 1500, "sent", "Ping", 38),
    plugin(
      4,
      start + 1840,
      "received",
      "mumcord.screen",
      { v: 1, kind: "announce", id: 7 },
      { sender: 3 },
    ),
    control(5, start + 2050, "received", "UserState", 64),
    media(
      6,
      start + 2600,
      "sent",
      214,
      {
        format: "media",
        version: null,
        channel: 0,
        control: 0,
        flags: null,
        keyframe: false,
      },
      '{\n  "kind": "screen"\n}',
    ),
    media(7, start + 2612, "sent", 20480, {
      format: "media",
      version: null,
      channel: 2,
      control: null,
      flags: 1,
      keyframe: true,
    }),
    media(8, start + 2645, "sent", 3210, {
      format: "media",
      version: null,
      channel: 2,
      control: null,
      flags: 0,
      keyframe: false,
    }),
    media(9, start + 2700, "sent", 4096, {
      format: "legacy",
      version: 2,
      channel: null,
      control: null,
      flags: null,
      keyframe: false,
    }),
  ];
}

/** The mock's capture switch and buffer. */
export function createMockInspector() {
  let capturing = false;
  let background = false;
  let packets: Packet[] = [];
  return {
    set(enabled: boolean) {
      capturing = enabled;
      packets = enabled ? samplePackets(Date.now()) : [];
    },
    setBackground(enabled: boolean) {
      background = enabled;
    },
    poll(after: number | null): PacketCapture {
      const counts = new Map<string, number>();
      for (const p of packets) {
        const key = `${p.kind}\n${p.direction}`;
        counts.set(key, (counts.get(key) ?? 0) + 1);
      }
      return {
        capturing,
        background,
        packets: packets.filter((p) => after === null || p.id > after),
        counters: Array.from(counts, ([key, count]) => {
          const [kind = "", direction] = key.split("\n");
          return {
            serverId: 1,
            kind,
            direction: direction === "sent" ? "sent" : "received",
            count,
          };
        }),
        voice: capturing
          ? [
              {
                serverId: 1,
                sent: 1520,
                received: 3044,
                rating: { rating: "fair", issue: "network" },
              },
            ]
          : [],
      };
    },
    clear() {
      packets = [];
    },
  };
}
