/**
 * Presentation helpers for the ban list (issue #254, FR-10.51). The engine hands out the full
 * values; the window shows fingerprints shortened and addresses masked by default, because IP
 * addresses and certificate hashes are personal data shown only in this window (FR-10.65).
 */
import type { BanEntry } from "../../api";

/** Server text is untrusted: render it as plain text, without control characters (FR-10.67). */
export function plain(value: string | null): string {
  return (value ?? "").replace(/\p{C}/gu, "").slice(0, 200);
}

/** Fingerprint as the window shows it: `a3f9…c21e` (FR-10.51). */
export function shortHash(hash: string | null): string {
  if (!hash) return "";
  return hash.length <= 8 ? hash : `${hash.slice(0, 4)}…${hash.slice(-4)}`;
}

/**
 * Address with the personal part hidden: the first three groups of an IPv4, the first four groups
 * of an IPv6, then `*` (FR-10.51, spec 13 A3). A single address keeps its last group hidden too,
 * so the row shows `203.0.113.*` and the "Show" button reveals it.
 */
export function maskAddress(address: string, mask: number): string {
  if (address.includes(":")) {
    const groups = address.split(":");
    return `${groups.slice(0, Math.min(4, groups.length - 1)).join(":")}:*`;
  }
  const groups = address.split(".");
  const shown = Math.max(1, Math.min(3, Math.ceil(Math.min(mask, 32) / 8) - 1));
  return `${groups.slice(0, shown).join(".")}.*`;
}

/** When a ban ends; `null` for a permanent one or an unreadable start. */
export function endTime(entry: BanEntry): Date | null {
  if (!entry.start || entry.durationSecs === 0) return null;
  const start = new Date(entry.start);
  if (Number.isNaN(start.getTime())) return null;
  return new Date(start.getTime() + entry.durationSecs * 1000);
}

/** Whether the window can send this as an address (FR-10.51). */
export function looksLikeAddress(value: string): boolean {
  const groups = value.split(".");
  if (groups.length === 4) return groups.every((g) => /^\d{1,3}$/.test(g) && Number(g) <= 255);
  return value.includes(":") && /^[0-9a-f:]+$/i.test(value);
}

export type SortColumn = "name" | "certHash" | "address" | "reason" | "start" | "ends";

/** The "Ends" order: permanent bans last, expired ones first (FR-10.53). */
export function endsOrder(entry: BanEntry): number {
  const end = endTime(entry);
  return end === null ? Number.MAX_SAFE_INTEGER : end.getTime();
}

export type BanFilter = "all" | "active" | "expired";

export function filterBans(entries: readonly BanEntry[], filter: BanFilter): BanEntry[] {
  if (filter === "all") return [...entries];
  return entries.filter((entry) => (filter === "expired" ? entry.expired : !entry.expired));
}

/** Instant search: name, IP, fingerprint and reason (FR-10.53). */
export function searchBans(entries: readonly BanEntry[], query: string): BanEntry[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...entries];
  return entries.filter((entry) =>
    [entry.name, entry.address, entry.certHash, entry.reason].some((value) =>
      (value ?? "").toLowerCase().includes(needle),
    ),
  );
}

export function sortBans(
  entries: readonly BanEntry[],
  sort: { column: SortColumn; ascending: boolean },
): BanEntry[] {
  const direction = sort.ascending ? 1 : -1;
  const text = (value: string | null) => (value ?? "").toLowerCase();
  return [...entries].sort((a, b) => {
    if (sort.column === "ends") return (endsOrder(a) - endsOrder(b)) * direction;
    if (sort.column === "start") {
      return (new Date(a.start ?? 0).getTime() - new Date(b.start ?? 0).getTime()) * direction;
    }
    return text(a[sort.column]).localeCompare(text(b[sort.column])) * direction;
  });
}
