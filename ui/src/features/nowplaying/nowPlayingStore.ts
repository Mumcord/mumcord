/**
 * What other people on the server are listening to (FR-1.58), from `nowPlayingChanged`. Tracks
 * belong to one connection: they are dropped on every new sync, on disconnect and when a person
 * leaves.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import { useShownServerId } from "../channels/serverStore";
import type { LiveTrack } from "./progress";

type Tracks = ReadonlyMap<number, LiveTrack>;

const EMPTY: Tracks = new Map();

/** Per server id; each server's map is keyed by session. */
let tracks = new Map<number, Tracks>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(serverId: number, next: Tracks): void {
  tracks = new Map(tracks);
  if (next.size === 0) tracks.delete(serverId);
  else tracks.set(serverId, next);
  for (const listener of listeners) listener();
}

function without(serverId: number, session: number): void {
  const current = tracks.get(serverId);
  if (!current?.has(session)) return;
  const next = new Map(current);
  next.delete(session);
  set(serverId, next);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  tracks = new Map();
  unsubscribers = [
    api.on("nowPlayingChanged", ({ serverId, session, track }) => {
      if (track === null) {
        without(serverId, session);
        return;
      }
      const next = new Map(tracks.get(serverId) ?? EMPTY);
      // The position is anchored to the local arrival time, not the sender's clock.
      next.set(session, { track, atMs: Date.now() });
      set(serverId, next);
    }),
    api.on("serverSynced", ({ snapshot }) => {
      set(snapshot.serverId, EMPTY);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "connected") set(serverId, EMPTY);
    }),
    api.on("userRemoved", ({ serverId, session }) => {
      without(serverId, session);
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
export function useNowPlaying(session: number, serverId?: number | null): LiveTrack | null {
  const shown = useShownServerId();
  const server = serverId === undefined ? shown : serverId;
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return (server === null ? undefined : tracks.get(server)?.get(session)) ?? null;
  });
}

/** Test helper: forgets the binding and all tracks. */
export function resetNowPlayingStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  tracks = new Map();
}
