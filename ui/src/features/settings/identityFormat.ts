/**
 * Display helpers of the identity manager (issue #148): Mumble fingerprints are SHA-1 hashes as
 * 40 lowercase hex characters; the UI groups them in byte pairs.
 */
import type { Identity, SavedServer } from "../../api";

/** Every byte pair of a certificate hash, e.g. "4F:A2:9C:…". */
export function fullFingerprint(certHash: string): string {
  return (certHash.match(/.{1,2}/g) ?? []).join(":").toUpperCase();
}

/** Short fingerprint for lists: the first eight pairs, an ellipsis, the last two. */
export function shortFingerprint(certHash: string): string {
  const groups = (certHash.match(/.{1,2}/g) ?? []).map((part) => part.toUpperCase());
  if (groups.length <= 10) return groups.join(":");
  return [...groups.slice(0, 8), "…", ...groups.slice(-2)].join(":");
}

/** The letter in the round avatar: first character of the label, uppercased. */
export function identityInitial(label: string): string {
  return (Array.from(label.trim())[0] ?? "?").toUpperCase();
}

/** Servers whose next connect uses `identity`: their own choice, or the default. */
export function serversFor(identity: Identity, servers: readonly SavedServer[]): SavedServer[] {
  return servers.filter((server) =>
    identity.isDefault
      ? server.identityId === null || server.identityId === identity.id
      : server.identityId === identity.id,
  );
}

/** Colour class of the avatar; the same identity always gets the same one. */
export function identityTone(id: number): string {
  return `identity-card__avatar--tone${String(id % 6)}`;
}
