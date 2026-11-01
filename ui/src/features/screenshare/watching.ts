/**
 * Which screen share the user has open in the main area (FR-2.45); one at a time. The viewer
 * closes by itself when the share ends or leaves the user's channel (`screenSharesChanged`)
 * and when the connection it belongs to drops or resyncs.
 */
import { useSyncExternalStore } from "react";
import { closeMedia, openMedia } from "../media/mediaView";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import { releaseKeptWatch } from "./frameHandover";
import { StreamMeter } from "./streamStats";

export interface WatchedShare {
  serverId: number;
  /** The sharing person. */
  session: number;
}

let watched: WatchedShare | null = null;
// One meter per watch, so the viewer's chips and the panel's own row show the same figures.
let meter = new StreamMeter();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: WatchedShare | null): void {
  if (next === watched) return;
  // A watch kept open for the pop-out ends with the share the user watched (issue #164).
  releaseKeptWatch(next);
  watched = next;
  meter = new StreamMeter();
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

/** The share open in the main area, or `null`, for code outside React. */
export function watchedShare(): WatchedShare | null {
  ensureBound();
  return watched;
}

/** Opens a person's share in the main area, replacing any other one ("Watch"). */
export function startWatching(serverId: number, session: number): void {
  ensureBound();
  if (watched?.serverId === serverId && watched.session === session) return;
  set({ serverId, session });
  // Issue #163: a watched share opens as the picture in the main column.
  openMedia("share");
}

/** Closes the viewer ("Stop watching"); the viewer then tells the engine. */
export function stopWatching(): void {
  closeMedia("share");
  set(null);
}

/** The share open in the main area, or `null`. */
export function useWatchedShare(): WatchedShare | null {
  return useSyncExternalStore(subscribe, watchedShare);
}

/** Counts the frames of the share open now (issue #163). */
export function watchMeter(): StreamMeter {
  return meter;
}

/** Test helper: forgets the binding and the open share. */
export function resetWatchingStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  watched = null;
  meter = new StreamMeter();
}
