import type { InstalledPlugin } from "../../../api";

/** The permission that lets a plugin send its data through the relay (FR-9.63). */
export const RELAY = "peers.relay";

/** Whether `plugin` asks for `peers.relay`; only those can send their data through the relay. */
export function asksForRelay(plugin: InstalledPlugin): boolean {
  return plugin.required.includes(RELAY) || plugin.optional.some((o) => o.id === RELAY);
}
