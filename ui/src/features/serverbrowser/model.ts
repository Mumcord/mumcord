import type { PublicServer, PublicServerPing } from "../../api";

/** What the paced ping of one listed server produced so far. */
export type PingState =
  { kind: "pending" } | { kind: "unreachable" } | ({ kind: "answered" } & PublicServerPing);

export type SortKey = "name" | "country" | "ping" | "users";
export interface Sort {
  key: SortKey;
  ascending: boolean;
}

export interface Filters {
  query: string;
  /** ISO code, or null for any country. */
  country: string | null;
  /** Highest ping in ms, or null for any ping. */
  maxPing: number | null;
  hideEmpty: boolean;
  hideFull: boolean;
}

export const NO_FILTERS: Filters = {
  query: "",
  country: null,
  maxPing: null,
  hideEmpty: false,
  hideFull: false,
};

/** The choices of the ping filter, in ms. */
export const PING_LIMITS = [50, 100, 200] as const;

const PENDING: PingState = { kind: "pending" };

export function serverKey(server: Pick<PublicServer, "host" | "port">): string {
  return `${server.host.toLowerCase()}:${String(server.port)}`;
}

export function pingOf(pings: ReadonlyMap<string, PingState>, server: PublicServer): PingState {
  return pings.get(serverKey(server)) ?? PENDING;
}

/** Colour band of a ping, as in the mockup: green, amber, red. */
export function pingTone(ms: number): "good" | "fair" | "poor" {
  if (ms < 100) return "good";
  if (ms < 125) return "fair";
  return "poor";
}

export function isEmpty(ping: PingState): boolean {
  return ping.kind === "answered" && ping.users === 0;
}

export function isFull(ping: PingState): boolean {
  return ping.kind === "answered" && ping.maxUsers > 0 && ping.users >= ping.maxUsers;
}

/**
 * Whether a server passes the filters. Servers whose ping is still pending stay visible: only
 * rows on screen are pinged, so hiding them would mean they are never measured.
 */
function passes(server: PublicServer, ping: PingState, filters: Filters): boolean {
  const query = filters.query.trim().toLocaleLowerCase();
  if (
    query !== "" &&
    ![server.name, server.host, server.country, server.countryCode].some((field) =>
      field.toLocaleLowerCase().includes(query),
    )
  ) {
    return false;
  }
  if (filters.country !== null && server.countryCode !== filters.country) return false;
  if (filters.maxPing !== null) {
    if (ping.kind === "unreachable") return false;
    if (ping.kind === "answered" && ping.pingMs > filters.maxPing) return false;
  }
  if (filters.hideEmpty && isEmpty(ping)) return false;
  if (filters.hideFull && isFull(ping)) return false;
  return true;
}

/** Pinged servers first, by ping or users; the rest after them in name order. */
function measured(ping: PingState, key: "ping" | "users"): number | null {
  if (ping.kind !== "answered") return null;
  return key === "ping" ? ping.pingMs : ping.users;
}

/** The servers to show, filtered and sorted. */
export function shownServers(
  servers: readonly PublicServer[],
  pings: ReadonlyMap<string, PingState>,
  filters: Filters,
  sort: Sort,
): PublicServer[] {
  const byName = (a: PublicServer, b: PublicServer) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  const direction = sort.ascending ? 1 : -1;
  const compare = (a: PublicServer, b: PublicServer): number => {
    switch (sort.key) {
      case "name":
        return direction * byName(a, b);
      case "country":
        return direction * a.countryCode.localeCompare(b.countryCode) || byName(a, b);
      case "ping":
      case "users": {
        const x = measured(pingOf(pings, a), sort.key);
        const y = measured(pingOf(pings, b), sort.key);
        if (x === null || y === null) {
          if (x === y) return byName(a, b);
          return x === null ? 1 : -1;
        }
        return direction * (x - y) || byName(a, b);
      }
    }
  };
  return servers.filter((s) => passes(s, pingOf(pings, s), filters)).sort(compare);
}

/** The countries in the list, by name, for the country filter. */
export function countriesOf(servers: readonly PublicServer[]): { code: string; name: string }[] {
  const names = new Map<string, string>();
  for (const server of servers) {
    if (server.countryCode !== "" && !names.has(server.countryCode)) {
      names.set(server.countryCode, server.country || server.countryCode);
    }
  }
  return [...names]
    .map(([code, name]) => ({ code, name }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** A new sort after a click on a column header: the same column flips, another one starts. */
export function nextSort(current: Sort, key: SortKey): Sort {
  if (current.key === key) return { key, ascending: !current.ascending };
  // Most users first is what people look for; the other columns start low to high.
  return { key, ascending: key !== "users" };
}
