/**
 * What other people on the server are playing (FR-1.59), from `nowGamingChanged`. Games belong
 * to one connection: they are dropped on every new sync, on disconnect and when a person leaves.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import type { LiveGame } from "./playTime";

type Games = ReadonlyMap<number, LiveGame>;

const EMPTY: Games = new Map();

let games: Games = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: Games): void {
  games = next;
  for (const listener of listeners) listener();
}

function without(session: number): void {
  if (!games.has(session)) return;
  const next = new Map(games);
  next.delete(session);
  set(next);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  games = EMPTY;
  unsubscribers = [
    api.on("nowGamingChanged", ({ session, game }) => {
      if (game === null) {
        without(session);
        return;
      }
      const next = new Map(games);
      // The play time is anchored to the local arrival time, not the sender's clock.
      next.set(session, { game, atMs: Date.now() });
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

/** The game a person shares, or `null`. */
export function useNowGaming(session: number): LiveGame | null {
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return games.get(session) ?? null;
  });
}

/** Test helper: forgets the binding and all games. */
export function resetNowGamingStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  games = EMPTY;
}
