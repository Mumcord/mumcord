import { describe, expect, it } from "vitest";
import type { PublicServer } from "../../api";
import {
  NO_FILTERS,
  countriesOf,
  nextSort,
  pingTone,
  serverKey,
  shownServers,
  type PingState,
} from "./model";

const server = (name: string, countryCode: string, country = countryCode): PublicServer => ({
  name,
  host: `${name.toLowerCase().replace(/ /g, "")}.example.org`,
  port: 64738,
  country,
  countryCode,
});

const ALPHA = server("Alpha", "DE", "Germany");
const BRAVO = server("Bravo", "SE", "Sweden");
const CHARLIE = server("Charlie", "DE", "Germany");
const DELTA = server("Delta", "", "");
const ALL = [CHARLIE, DELTA, BRAVO, ALPHA];

const answered = (pingMs: number, users: number, maxUsers: number): PingState => ({
  kind: "answered",
  pingMs,
  users,
  maxUsers,
});

const PINGS = new Map<string, PingState>([
  [serverKey(ALPHA), answered(30, 0, 10)],
  [serverKey(BRAVO), answered(150, 10, 10)],
  [serverKey(CHARLIE), { kind: "unreachable" }],
]);

const names = (list: PublicServer[]) => list.map((s) => s.name);
const byName = { key: "name", ascending: true } as const;

describe("server browser model", () => {
  it("sorts by name and flips the direction on a second click", () => {
    expect(names(shownServers(ALL, PINGS, NO_FILTERS, byName))).toEqual([
      "Alpha",
      "Bravo",
      "Charlie",
      "Delta",
    ]);
    const flipped = nextSort(byName, "name");
    expect(names(shownServers(ALL, PINGS, NO_FILTERS, flipped))[0]).toBe("Delta");
  });

  it("sorts by ping with servers that did not answer last", () => {
    const sort = nextSort(byName, "ping");
    expect(sort.ascending).toBe(true);
    expect(names(shownServers(ALL, PINGS, NO_FILTERS, sort))).toEqual([
      "Alpha",
      "Bravo",
      "Charlie",
      "Delta",
    ]);
  });

  it("starts the users column with the busiest server", () => {
    const sort = nextSort(byName, "users");
    expect(sort.ascending).toBe(false);
    expect(names(shownServers(ALL, PINGS, NO_FILTERS, sort)).slice(0, 2)).toEqual([
      "Bravo",
      "Alpha",
    ]);
  });

  it("searches name, address and country", () => {
    const find = (query: string) =>
      names(shownServers(ALL, PINGS, { ...NO_FILTERS, query }, byName));
    expect(find("brav")).toEqual(["Bravo"]);
    expect(find("CHARLIE.EXAMPLE")).toEqual(["Charlie"]);
    expect(find("germany")).toEqual(["Alpha", "Charlie"]);
  });

  it("filters by country", () => {
    expect(names(shownServers(ALL, PINGS, { ...NO_FILTERS, country: "SE" }, byName))).toEqual([
      "Bravo",
    ]);
  });

  it("filters by ping but keeps servers that are not measured yet", () => {
    expect(names(shownServers(ALL, PINGS, { ...NO_FILTERS, maxPing: 100 }, byName))).toEqual([
      "Alpha",
      "Delta",
    ]);
  });

  it("hides empty and full servers only once their figures are known", () => {
    expect(
      names(shownServers(ALL, PINGS, { ...NO_FILTERS, hideEmpty: true, hideFull: true }, byName)),
    ).toEqual(["Charlie", "Delta"]);
  });

  it("lists each country once, by name", () => {
    expect(countriesOf(ALL)).toEqual([
      { code: "DE", name: "Germany" },
      { code: "SE", name: "Sweden" },
    ]);
  });

  it("colours pings like the mockup", () => {
    expect(pingTone(38)).toBe("good");
    expect(pingTone(118)).toBe("fair");
    expect(pingTone(131)).toBe("poor");
  });
});
