/**
 * Packet list logic of the packet inspector, free of React: merging polled packets, filters,
 * display formats and the JSON and Markdown exports.
 */
import type { Packet as ApiPacket, PacketDirection } from "../../api";

/**
 * A captured packet as the inspector uses it. The bindings type every float as nullable; the
 * engine always sets the capture time, and `fromApi` makes that explicit.
 */
export type Packet = Omit<ApiPacket, "atMs"> & { atMs: number };

export function fromApi(packet: ApiPacket): Packet {
  return { ...packet, atMs: packet.atMs ?? 0 };
}

/** Packets the UI keeps, like the engine's ring buffer. */
export const MAX_PACKETS = 2000;

/** Plain-language names of Mumcord's plugin data, by data id (see the wiki's protocol page). */
export const PLUGIN_KINDS: Readonly<Record<string, string>> = {
  "mumcord.hello": "hello",
  "mumcord.history": "history",
  "mumcord.image": "image",
  "mumcord.nowgaming": "nowgaming",
  "mumcord.nowplaying": "nowplaying",
  "mumcord.p2p": "p2p",
  "mumcord.screen": "screen",
  "mumcord.watch": "watch",
};

/** Translation key of a packet type's plain name, below `inspector.kinds`. */
export function kindKey(packet: Pick<Packet, "kind" | "pluginData">): string {
  if (!packet.pluginData) return "mumble";
  return PLUGIN_KINDS[packet.kind] ?? "plugin";
}

export function mergePackets(current: readonly Packet[], added: readonly Packet[]): Packet[] {
  if (added.length === 0) return current as Packet[];
  const merged = [...current, ...added];
  return merged.length > MAX_PACKETS ? merged.slice(merged.length - MAX_PACKETS) : merged;
}

export interface PacketFilter {
  serverId: number | null;
  direction: PacketDirection | null;
  kind: string | null;
}

export const NO_FILTER: PacketFilter = { serverId: null, direction: null, kind: null };

export function filterPackets(packets: readonly Packet[], filter: PacketFilter): Packet[] {
  return packets.filter(
    (p) =>
      (filter.serverId === null || p.serverId === filter.serverId) &&
      (filter.direction === null || p.direction === filter.direction) &&
      (filter.kind === null || p.kind === filter.kind),
  );
}

/** Every packet type in the list, plugin data first, each group sorted. */
export function packetKinds(packets: readonly Packet[]): string[] {
  const plugin = new Set<string>();
  const other = new Set<string>();
  for (const p of packets) (p.pluginData ? plugin : other).add(p.kind);
  return [...[...plugin].sort(), ...[...other].sort()];
}

const pad = (n: number, width = 2) => String(n).padStart(width, "0");

/** Local time with milliseconds, `HH:MM:SS.mmm`. */
export function formatTime(atMs: number): string {
  const d = new Date(atMs);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

/** Bytes, or KB with one decimal from 1 KB on. */
export function formatSize(bytes: number): string {
  return bytes < 1024 ? `${String(bytes)} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

/** Time since the previous packet: `+340 ms`, from one second on `+3.2 s`. */
export function formatGap(ms: number): string {
  return ms < 1000 ? `+${String(Math.round(ms))} ms` : `+${(ms / 1000).toFixed(1)} s`;
}

/** Hex bytes in rows of 16, offsets in front. */
export function formatHex(rawHex: string): string {
  const rows: string[] = [];
  for (let at = 0; at < rawHex.length; at += 32) {
    const bytes = rawHex.slice(at, at + 32).match(/../g) ?? [];
    rows.push(`${(at / 2).toString(16).padStart(4, "0")}  ${bytes.join(" ")}`);
  }
  return rows.join("\n");
}

/** Names the export and the list show for sessions and servers; `null` when unknown. */
export interface Names {
  user(serverId: number, session: number): string | null;
  server(serverId: number): string | null;
}

export interface ExportInfo {
  appVersion: string;
  os: string;
  exportedAtMs: number;
}

function peer(names: Names, serverId: number, session: number) {
  return { session, name: names.user(serverId, session) };
}

/** Complete, machine-readable export of the given packets. */
export function exportJson(packets: readonly Packet[], info: ExportInfo, names: Names): string {
  const first = packets[0];
  const last = packets[packets.length - 1];
  return JSON.stringify(
    {
      warning: "May contain personal data such as names and chat text.",
      app: "Mumcord",
      appVersion: info.appVersion,
      os: info.os,
      exportedAt: new Date(info.exportedAtMs).toISOString(),
      timeRange:
        first && last
          ? { from: new Date(first.atMs).toISOString(), to: new Date(last.atMs).toISOString() }
          : null,
      packets: packets.map((p) => ({
        id: p.id,
        time: new Date(p.atMs).toISOString(),
        localTime: formatTime(p.atMs),
        server: { id: p.serverId, name: names.server(p.serverId) },
        direction: p.direction,
        kind: p.kind,
        pluginData: p.pluginData,
        size: p.size,
        from: p.sender === null ? null : peer(names, p.serverId, p.sender),
        to: p.receivers.map((s) => peer(names, p.serverId, s)),
        pacer: p.delayMs === null ? null : { delayMs: p.delayMs, tokensLeft: p.tokensLeft },
        decoded: p.decoded === null ? null : (JSON.parse(p.decoded) as unknown),
        rawHex: p.rawHex,
      })),
    },
    null,
    2,
  );
}

/** "Alice (session 12)", or "session 12" for someone who left. */
export function peerLabel(names: Names, serverId: number, session: number): string {
  const name = names.user(serverId, session);
  return name === null ? `session ${String(session)}` : `${name} (session ${String(session)})`;
}

/** Table cells may not contain pipes or line breaks. */
const cell = (text: string) => text.replace(/\|/g, "\\|").replace(/\s*\n\s*/g, " ");

function peersText(p: Packet, names: Names): string {
  if (p.direction === "received") {
    return p.sender === null ? "" : `← ${peerLabel(names, p.serverId, p.sender)}`;
  }
  return p.receivers.length === 0
    ? ""
    : `→ ${p.receivers.map((s) => peerLabel(names, p.serverId, s)).join(", ")}`;
}

function pacerText(p: Packet): string {
  if (p.delayMs === null) return "";
  const delay = p.delayMs > 0 ? `delayed ${String(p.delayMs)} ms` : "not delayed";
  return `${delay}, ${String(p.tokensLeft ?? 0)} tokens left`;
}

function headerLines(packets: readonly Packet[], info: ExportInfo): string[] {
  const first = packets[0];
  const last = packets[packets.length - 1];
  return [
    "# Mumcord packet export",
    "",
    "> May contain personal data such as names and chat text. Check before sharing.",
    "",
    `- App version: ${info.appVersion}`,
    `- OS: ${info.os}`,
    `- Exported: ${new Date(info.exportedAtMs).toISOString()}`,
    `- Time range: ${first && last ? `${formatTime(first.atMs)} – ${formatTime(last.atMs)}` : "–"}`,
    `- Packets: ${String(packets.length)}`,
  ];
}

/** Readable export for pasting into an issue: a table plus the decoded content per packet. */
export function exportMarkdown(packets: readonly Packet[], info: ExportInfo, names: Names): string {
  const lines = [
    ...headerLines(packets, info),
    "",
    "| Time | Direction | Type | Size | From / To | Pacer |",
    "|---|---|---|---|---|---|",
    ...packets.map(
      (p) =>
        `| ${formatTime(p.atMs)} | ${p.direction === "sent" ? "↑ sent" : "↓ received"} | ${cell(p.kind)} | ${formatSize(p.size)} | ${cell(peersText(p, names))} | ${pacerText(p)} |`,
    ),
  ];
  const decoded = packets.filter((p) => p.decoded !== null);
  if (decoded.length > 0) {
    lines.push("", "## Decoded content");
    for (const p of decoded) lines.push("", ...packetBlock(p, names, null));
  }
  return `${lines.join("\n")}\n`;
}

/** One packet as Markdown, for the copy button; `previousAtMs` adds the gap. */
export function packetBlock(p: Packet, names: Names, previousAtMs: number | null): string[] {
  const facts = [
    `${formatTime(p.atMs)} ${p.direction === "sent" ? "↑ sent" : "↓ received"} ${p.kind}, ${formatSize(p.size)}`,
    peersText(p, names),
    pacerText(p),
    previousAtMs === null ? "" : `gap ${formatGap(p.atMs - previousAtMs)}`,
  ].filter((line) => line !== "");
  const lines = [`### ${facts[0] ?? ""}`, "", ...facts.slice(1).map((f) => `- ${f}`)];
  if (p.decoded !== null) lines.push("", ...fenced("json", p.decoded));
  if (p.rawHex !== "") lines.push("", ...fenced("text", formatHex(p.rawHex)));
  return lines;
}

/** A code block whose fence is longer than any backtick run in the (untrusted) text. */
function fenced(language: string, text: string): string[] {
  const longest = Math.max(0, ...(text.match(/`+/g) ?? []).map((run) => run.length));
  const fence = "`".repeat(Math.max(3, longest + 1));
  return [`${fence}${language}`, text, fence];
}
