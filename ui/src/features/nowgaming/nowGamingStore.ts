/**
 * What other people on the server are playing (FR-1.59), from `nowGamingChanged`. Games belong
 * to one connection: they are dropped on every new sync, on disconnect and when a person leaves.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import { useShownServerId } from "../channels/serverStore";
import type { LiveGame } from "./playTime";

type Games = ReadonlyMap<number, LiveGame>;

const EMPTY: Games = new Map();

/** Per server id; each server's map is keyed by session. */
let games = new Map<number, Games>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(serverId: number, next: Games): void {
  games = new Map(games);
  if (next.size === 0) games.delete(serverId);
  else games.set(serverId, next);
  for (const listener of listeners) listener();
}

function without(serverId: number, session: number): void {
  const current = games.get(serverId);
  if (!current?.has(session)) return;
  const next = new Map(current);
  next.delete(session);
  set(serverId, next);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  games = new Map();
  unsubscribers = [
    api.on("nowGamingChanged", ({ serverId, session, game }) => {
      if (game === null) {
        without(serverId, session);
        return;
      }
      const next = new Map(games.get(serverId) ?? EMPTY);
      // The play time is anchored to the local arrival time, not the sender's clock.
      next.set(session, { game, atMs: Date.now() });
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

/** The game a person shares, or `null`. */
export function useNowGaming(session: number, serverId?: number | null): LiveGame | null {
  const shown = useShownServerId();
  const server = serverId === undefined ? shown : serverId;
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return (server === null ? undefined : games.get(server)?.get(session)) ?? null;
  });
}

/** Test helper: forgets the binding and all games. */
export function resetNowGamingStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  games = new Map();
}
