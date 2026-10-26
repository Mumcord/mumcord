/**
 * Which screen share the user has open in the main area (FR-2.45); one at a time. The viewer
 * closes by itself when the share ends or leaves the user's channel (`screenSharesChanged`)
 * and when the connection it belongs to drops or resyncs.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";

export interface WatchedShare {
  serverId: number;
  /** The sharing person. */
  session: number;
}

let watched: WatchedShare | null = null;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: WatchedShare | null): void {
  if (next === watched) return;
  watched = next;
  for (const listener of listeners) listener();
}

function closeOn(serverId: number): void {
  if (watched?.serverId === serverId) set(null);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  watched = null;
  unsubscribers = [
    api.on("screenSharesChanged", ({ serverId, shares }) => {
      if (watched?.serverId !== serverId) return;
      const { session } = watched;
      if (!shares.some((share) => share.session === session)) set(null);
    }),
    api.on("serverSynced", ({ snapshot }) => {
      closeOn(snapshot.serverId);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "connected") closeOn(serverId);
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

function snapshot(): WatchedShare | null {
  ensureBound();
  return watched;
}

/** Opens a person's share in the main area, replacing any other one ("Watch"). */
export function startWatching(serverId: number, session: number): void {
  ensureBound();
  if (watched?.serverId === serverId && watched.session === session) return;
  set({ serverId, session });
}

/** Closes the viewer ("Stop watching"); the viewer then tells the engine. */
export function stopWatching(): void {
  set(null);
}

/** The share open in the main area, or `null`. */
export function useWatchedShare(): WatchedShare | null {
  return useSyncExternalStore(subscribe, snapshot);
}

/** Test helper: forgets the binding and the open share. */
export function resetWatchingStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  watched = null;
}
