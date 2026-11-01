/**
 * The screen shares in the user's voice channel per server (FR-2.42–2.45), from
 * `screenSharesChanged`. Shares belong to one connection: they are dropped on every new sync and
 * on disconnect, and the list is read again after a sync in case its event came first.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type ScreenShare, type Unsubscribe } from "../../api";

const NONE: readonly ScreenShare[] = [];

let shares = new Map<number, readonly ScreenShare[]>();
// When each share object arrived: its `elapsedMs` is counted up to that moment.
let arrived = new WeakMap<ScreenShare, number>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(serverId: number, list: readonly ScreenShare[]): void {
  shares = new Map(shares);
  const now = Date.now();
  for (const share of list) if (!arrived.has(share)) arrived.set(share, now);
  if (list.length === 0) shares.delete(serverId);
  else shares.set(serverId, list);
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  shares = new Map();
  unsubscribers = [
    api.on("screenSharesChanged", ({ serverId, shares: list }) => {
      set(serverId, list);
    }),
    api.on("serverSynced", ({ snapshot }) => {
      const { serverId } = snapshot;
      set(serverId, NONE);
      api.screenShareList(serverId).then(
        (list) => {
          if (boundApi === api) set(serverId, list);
        },
        (error: unknown) => {
          console.error("Reading the screen shares failed", error);
        },
      );
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "connected") set(serverId, NONE);
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

function sharesOf(serverId: number | null): readonly ScreenShare[] {
  ensureBound();
  return (serverId === null ? undefined : shares.get(serverId)) ?? NONE;
}

/**
 * Takes the shares the engine listed for a window that opened later (the screen share pop-out,
 * issue #164); a `screenSharesChanged` that arrived first wins.
 */
export function seedScreenShares(serverId: number, list: readonly ScreenShare[]): void {
  ensureBound();
  if (!shares.has(serverId)) set(serverId, list);
}

/** Every share in the user's voice channel on `serverId`, including the own one. */
export function useScreenShares(serverId: number | null): readonly ScreenShare[] {
  return useSyncExternalStore(subscribe, () => sharesOf(serverId));
}

/** The share of `session` on `serverId`, or `null` when that person isn't sharing. */
export function useScreenShareOf(
  serverId: number | null,
  session: number | null,
): ScreenShare | null {
  return useSyncExternalStore(
    subscribe,
    () => sharesOf(serverId).find((share) => share.session === session) ?? null,
  );
}

/** Whether `session` is sharing on `serverId`; cheap enough for every tree and member row. */
export function useIsLive(serverId: number | null, session: number): boolean {
  return useSyncExternalStore(subscribe, () =>
    sharesOf(serverId).some((share) => share.session === session),
  );
}

/** When `share` started, as `Date.now()` milliseconds (issue #163: "since 21:04", the elapsed clock). */
export function shareStartedAt(share: ScreenShare): number {
  return (arrived.get(share) ?? Date.now()) - share.elapsedMs;
}

/** Test helper: forgets the binding and all shares. */
export function resetScreenShareStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  shares = new Map();
  arrived = new WeakMap();
}
