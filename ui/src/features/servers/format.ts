import type { SavedServer } from "../../api";

export const DEFAULT_PORT = 64738;

/** Up to two initials for a server icon: first letters of the first two words. */
export function initials(label: string): string {
  const words = label.trim().split(/\s+/).filter(Boolean);
  const letters = words.slice(0, 2).map((word) => Array.from(word)[0] ?? "");
  return letters.join("").toUpperCase() || "?";
}

/** The label, or host:port for an entry without one (an id is never shown). */
export function displayName(server: Pick<SavedServer, "label" | "address" | "port">): string {
  return server.label.trim() || `${server.address}:${String(server.port)}`;
}

export function isMumbleLink(value: string): boolean {
  return value.trim().toLowerCase().startsWith("mumble://");
}

/**
 * Splits "host", "host:port" or "[ipv6]:port"; `port` is null when none was typed. A bare IPv6 address (several colons) is kept as
 * the host so it is not mistaken for a port.
 */
export function splitAddress(value: string): { address: string; port: number | null } {
  const trimmed = value.trim();
  const bracketed = /^\[(.+)\](?::(\d{1,5}))?$/.exec(trimmed);
  if (bracketed?.[1]) {
    return { address: bracketed[1], port: bracketed[2] ? Number(bracketed[2]) : null };
  }
  const hostPort = /^([^:]+):(\d{1,5})$/.exec(trimmed);
  if (hostPort?.[1] && hostPort[2]) return { address: hostPort[1], port: Number(hostPort[2]) };
  return { address: trimmed, port: null };
}
