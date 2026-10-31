import type { PublicServer, PublicServerPing } from "../bindings/commands";

/**
 * Fictional public servers for the mock engine (issue #68), modelled on the mockup. Addresses use
 * `example.org`; nothing here is contacted.
 */
type Row = [
  name: string,
  sub: string,
  country: string,
  code: string,
  ping: number,
  users: number,
  max: number,
];

const ROWS: readonly Row[] = [
  ["Nordic Gamers", "nordic", "Sweden", "SE", 38, 24, 60],
  ["Retro LAN", "retrolan", "Germany", "DE", 22, 12, 50],
  ["Quiet Library", "library", "Netherlands", "NL", 31, 3, 25],
  ["Pixel Harbor", "pixelharbor", "Finland", "FI", 47, 41, 100],
  ["Coffee Club", "coffee", "France", "FR", 52, 0, 30],
  ["Night Raiders", "raiders", "United States", "US", 118, 64, 64],
  ["Board Game Cafe", "boardgames", "Canada", "CA", 131, 64, 64],
  ["Silent Orbit", "orbit", "Poland", "PL", 44, 9, 40],
  ["Alpine Radio", "alpine", "Austria", "AT", 29, 7, 50],
  ["Lisbon Late Shift", "lisbon", "Portugal", "PT", 61, 15, 40],
  ["Tram Line 9", "tram9", "Czechia", "CZ", 35, 2, 20],
  ["Harbour Lights", "harbour", "Ireland", "IE", 48, 0, 16],
  ["Midnight Strategy", "strategy", "Germany", "DE", 25, 33, 80],
  ["Sunday Tabletop", "tabletop", "Italy", "IT", 57, 6, 30],
  ["Aurora Wing", "aurora", "Norway", "NO", 41, 18, 64],
  ["Canal Talk", "canal", "Belgium", "BE", 27, 4, 25],
  ["Old Lighthouse", "lighthouse", "Denmark", "DK", 0, 0, 0],
];

const DEMO: readonly (PublicServer & { answer: PublicServerPing | null })[] = ROWS.map(
  ([name, sub, country, countryCode, pingMs, users, maxUsers]) => ({
    name,
    host: `${sub}.example.org`,
    port: 64738,
    country,
    countryCode,
    // A ping of 0 stands for a server that does not answer.
    answer: pingMs === 0 ? null : { pingMs, users, maxUsers },
  }),
);

export function mockPublicServers(): PublicServer[] {
  return DEMO.map(({ name, host, port, country, countryCode }) => ({
    name,
    host,
    port,
    country,
    countryCode,
  }));
}

export function mockPublicServerPing(host: string, port: number): PublicServerPing | null {
  return DEMO.find((s) => s.host === host && s.port === port)?.answer ?? null;
}
