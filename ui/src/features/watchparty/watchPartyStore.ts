/**
 * The watch party in the user's channel per server (FR-2.39, FR-2.40), from `watchPartyChanged`.
 * A party belongs to one connection: it is dropped on every new sync and on disconnect. The
 * position is anchored to the local arrival time so the player can tell where the party is now.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe, type WatchParty } from "../../api";

/** Who added a video: their session, and the name the engine kept in case they left. */
export interface Adder {
  session: number;
  name: string | null;
}

export interface LiveParty {
  party: WatchParty;
  /** `Date.now()` when `party.positionMs` was current. */
  atMs: number;
  /**
   * Who added the playing video, where known (issue #163). The engine only lists what plays next,
   * so the adder is remembered from the playlist as the entry moves up; the first video is the host's.
   */
  currentAdder: Adder | null;
}

let parties = new Map<number, LiveParty>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function adderOf(party: WatchParty, previous: LiveParty | undefined): Adder | null {
  if (party.currentEntry === 0) return { session: party.host, name: null };
  const listed = [...party.playlist, ...(previous?.party.playlist ?? [])].find(
    (entry) => entry.id === party.currentEntry,
  );
  if (listed !== undefined) return { session: listed.addedBy, name: listed.addedByName };
  return previous?.party.currentEntry === party.currentEntry ? previous.currentAdder : null;
}

function set(serverId: number, party: WatchParty | null): void {
  const previous = parties.get(serverId);
  parties = new Map(parties);
  if (party === null) parties.delete(serverId);
  else parties.set(serverId, { party, atMs: Date.now(), currentAdder: adderOf(party, previous) });
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  parties = new Map();
  unsubscribers = [
    api.on("watchPartyChanged", ({ serverId, party }) => {
      set(serverId, party);
    }),
    api.on("serverSynced", ({ snapshot }) => {
      set(snapshot.serverId, null);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "connected") set(serverId, null);
    }),
  ];
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Takes a party the engine returned from a command, before (or instead of) its event. */
export function setWatchParty(serverId: number, party: WatchParty | null): void {
  ensureBound();
  set(serverId, party);
}

/** The party in the user's channel on `serverId`, or `null`. */
export function useWatchParty(serverId: number | null): LiveParty | null {
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return (serverId === null ? undefined : parties.get(serverId)) ?? null;
  });
}

/** Where a party is at `nowMs`, in milliseconds into the video. */
export function positionAt(live: LiveParty, nowMs: number): number {
  const { party, atMs } = live;
  return party.playing ? party.positionMs + Math.max(0, nowMs - atMs) : party.positionMs;
}

/** Test helper: forgets the binding and all parties. */
export function resetWatchPartyStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  parties = new Map();
}
