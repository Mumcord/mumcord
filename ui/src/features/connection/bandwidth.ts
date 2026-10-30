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

/** Packet length the client sends today (two 10 ms frames, as the official client does). */
const NORMAL_FRAMES = 2;
const LOW_LATENCY_FRAMES = 1;

export interface BandwidthEstimate {
  /** Voice bitrate plus packet headers, in kbit/s. */
  kbps: number;
  /** Extra header cost of 10 ms packets compared with the current ones. */
  lowLatencyExtraKbps: number;
  /** Share of the server limit, 0 to 1; `null` without a limit. */
  share: number | null;
}

/** Estimated bandwidth including packet headers (the figure the server limit covers). */
export function estimateBandwidth(stats: ConnectionStats): BandwidthEstimate {
  const tunnelled = stats.transport === "tcp";
  const kbps = stats.sendBitrateKbps + overheadKbps(NORMAL_FRAMES, tunnelled);
  const lowLatencyExtraKbps =
    overheadKbps(LOW_LATENCY_FRAMES, tunnelled) - overheadKbps(NORMAL_FRAMES, tunnelled);
  const limit = stats.serverMaxBandwidthKbps;
  return {
    kbps,
    lowLatencyExtraKbps,
    share: limit !== null && limit > 0 ? Math.min(kbps / limit, 1) : null,
  };
}
