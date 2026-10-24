/**
 * What other people on the server are listening to (FR-1.58), from `nowPlayingChanged`. Tracks
 * belong to one connection: they are dropped on every new sync, on disconnect and when a person
 * leaves.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import type { LiveTrack } from "./progress";

type Tracks = ReadonlyMap<number, LiveTrack>;

const EMPTY: Tracks = new Map();

let tracks: Tracks = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: Tracks): void {
  tracks = next;
  for (const listener of listeners) listener();
}

function without(session: number): void {
  if (!tracks.has(session)) return;
  const next = new Map(tracks);
  next.delete(session);
  set(next);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  tracks = EMPTY;
  unsubscribers = [
    api.on("nowPlayingChanged", ({ session, track }) => {
      if (track === null) {
        without(session);
        return;
      }
      const next = new Map(tracks);
      // The position is anchored to the local arrival time, not the sender's clock.
      next.set(session, { track, atMs: Date.now() });
      set(next);
    }),
    api.on("serverSynced", () => {
      set(EMPTY);
    }),
    api.on("connectionStateChanged", ({ state }) => {
      if (state.state !== "connected") set(EMPTY);
    }),
    api.on("userRemoved", ({ session }) => {
      without(session);
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

/** The track a person shares, or `null`. */
export function useNowPlaying(session: number): LiveTrack | null {
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return tracks.get(session) ?? null;
  });
}

/** Test helper: forgets the binding and all tracks. */
export function resetNowPlayingStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  tracks = EMPTY;
}
