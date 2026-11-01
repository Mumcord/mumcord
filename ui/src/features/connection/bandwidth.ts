import type { ConnectionStats } from "../../bindings/commands";

/**
 * Header bytes of one voice packet, mirroring `network_overhead_bps` in the codec crate: IP (20),
 * UDP (8), Mumble audio header (4 + 1 + 2) and one byte per 10 ms frame in the packet. A TCP
 * tunnel adds 12 bytes of framing.
 */
function overheadKbps(framesPerPacket: number, tunnelled: boolean): number {
  const bytes = 20 + 8 + 4 + 1 + 2 + framesPerPacket + (tunnelled ? 12 : 0);
  return (bytes * 800) / framesPerPacket / 1000;
}

/** Packet length in normal mode (two 10 ms frames, as the official client does). */
const NORMAL_FRAMES = 2;
const LOW_LATENCY_FRAMES = 1;

export interface BandwidthEstimate {
  /** Voice bitrate plus packet headers, in kbit/s. */
  kbps: number;
  /** Extra header cost of 10 ms packets compared with 20 ms ones. */
  lowLatencyExtraKbps: number;
  /** Share of the server limit (above 1 when over it); `null` without a limit. */
  share: number | null;
}

/**
 * Estimated bandwidth including packet headers (the figure the server limit covers), with the
 * packet length of the current mode.
 */
export function estimateBandwidth(stats: ConnectionStats, lowLatency = false): BandwidthEstimate {
  const tunnelled = stats.transport === "tcp";
  const frames = lowLatency ? LOW_LATENCY_FRAMES : NORMAL_FRAMES;
  const kbps = stats.sendBitrateKbps + overheadKbps(frames, tunnelled);
  const lowLatencyExtraKbps =
    overheadKbps(LOW_LATENCY_FRAMES, tunnelled) - overheadKbps(NORMAL_FRAMES, tunnelled);
  const limit = stats.serverMaxBandwidthKbps;
  return {
    kbps,
    lowLatencyExtraKbps,
    share: limit !== null && limit > 0 ? kbps / limit : null,
  };
}

/** From this share of the server limit the meter warns. */
export const BANDWIDTH_WARNING_SHARE = 0.8;

export type BandwidthLevel = "normal" | "warning" | "danger";

/** Meter colour: neutral below 80 % of the server limit, warning from 80 % up to it, danger above it. */
export function bandwidthLevel(share: number): BandwidthLevel {
  if (share > 1) return "danger";
  return share >= BANDWIDTH_WARNING_SHARE ? "warning" : "normal";
}
