/**
 * Packet inspector of the mock engine: a fixed set of fictional packets appears when capture
 * starts, so the inspector can be tried and photographed without a server.
 */
import type { Packet, PacketCapture } from "../bindings/commands";

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
  ];
}

/** The mock's capture switch and buffer. */
export function createMockInspector() {
  let capturing = false;
  let packets: Packet[] = [];
  return {
    set(enabled: boolean) {
      capturing = enabled;
      packets = enabled ? samplePackets(Date.now()) : [];
    },
    poll(after: number | null): PacketCapture {
      const counts = new Map<string, number>();
      for (const p of packets) {
        const key = `${p.kind}\n${p.direction}`;
        counts.set(key, (counts.get(key) ?? 0) + 1);
      }
      return {
        capturing,
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
        voice: capturing ? [{ serverId: 1, sent: 1520, received: 3044 }] : [],
      };
    },
    clear() {
      packets = [];
    },
  };
}
