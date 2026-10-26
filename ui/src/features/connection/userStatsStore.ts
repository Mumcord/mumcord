/**
 * Connection figures of other people (FR-2.25), from `userStatsChanged`. They belong to one
 * connection: they are dropped on every new sync, on disconnect and when a person leaves.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import type { UserStats } from "../../bindings/commands";
import { useShownServerId } from "../channels/serverStore";

/** A connection is weak above this ping (FR-2.26). */
export const WEAK_PING_MS = 250;
/** A connection is weak above this packet loss (FR-2.26). */
export const WEAK_LOSS_PERCENT = 5;

type Stats = ReadonlyMap<number, UserStats>;

const EMPTY: Stats = new Map();

/** Per server id; each server's map is keyed by session. */
let stats = new Map<number, Stats>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(serverId: number, next: Stats): void {
  stats = new Map(stats);
  if (next.size === 0) stats.delete(serverId);
  else stats.set(serverId, next);
  for (const listener of listeners) listener();
}

function without(serverId: number, session: number): void {
  const current = stats.get(serverId);
  if (!current?.has(session)) return;
  const next = new Map(current);
  next.delete(session);
  set(serverId, next);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  stats = new Map();
  unsubscribers = [
    api.on("userStatsChanged", ({ serverId, session, stats: next }) => {
      set(serverId, new Map(stats.get(serverId) ?? EMPTY).set(session, next));
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

/** The latest figures of a person, or `null` before the first answer. */
export function useUserStats(session: number, serverId?: number | null): UserStats | null {
  const shown = useShownServerId();
  const server = serverId === undefined ? shown : serverId;
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return (server === null ? undefined : stats.get(server)?.get(session)) ?? null;
  });
}

/** Ping above 250 ms or packet loss above 5 % (FR-2.26); unknown values never count. */
export function isWeakConnection(figures: UserStats | null): boolean {
  if (!figures) return false;
  return (
    (figures.pingMs !== null && figures.pingMs > WEAK_PING_MS) ||
    (figures.lossPercent !== null && figures.lossPercent > WEAK_LOSS_PERCENT)
  );
}

export type ConnectionLevel = "good" | "fair" | "weak" | "unknown";

/**
 * Weak per FR-2.26, fair at half of those limits, otherwise good (FR-2.25b). Unknown while
 * neither ping nor loss is known: a missing figure says nothing about the quality.
 */
export function connectionLevel(figures: UserStats | null): ConnectionLevel {
  if (!figures || (figures.pingMs === null && figures.lossPercent === null)) return "unknown";
  if (isWeakConnection(figures)) return "weak";
  const fair =
    (figures.pingMs !== null && figures.pingMs > WEAK_PING_MS / 2) ||
    (figures.lossPercent !== null && figures.lossPercent > WEAK_LOSS_PERCENT / 2);
  return fair ? "fair" : "good";
}

/** Test helper: forgets the binding and all figures. */
export function resetUserStatsStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  stats = new Map();
}
