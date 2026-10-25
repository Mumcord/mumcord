/**
 * Friends with their online state (FR-1.64), as the engine reports them: online first. One store
 * for the app; it binds to the active engine client and re-binds when tests swap it.
 */
import { useSyncExternalStore } from "react";
import { getApi, type Friend, type MumcordApi, type Unsubscribe } from "../../api";

let friends: readonly Friend[] = [];
let boundApi: MumcordApi | undefined;
let unsubscribe: Unsubscribe | undefined;
let generation = 0;
/** Set once a `friendsChanged` arrived, so a late initial load cannot overwrite it. */
let changedSinceLoad = false;
const listeners = new Set<() => void>();

function set(next: readonly Friend[]): void {
  friends = next;
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  unsubscribe?.();
  boundApi = api;
  friends = [];
  changedSinceLoad = false;
  const mine = ++generation;
  unsubscribe = api.on("friendsChanged", ({ friends: next }) => {
    changedSinceLoad = true;
    set(next);
  });
  api.friendsList().then(
    (list) => {
      if (generation === mine && !changedSinceLoad) set(list);
    },
    (error: unknown) => {
      console.error("Loading the friends failed", error);
    },
  );
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

function getSnapshot(): readonly Friend[] {
  ensureBound();
  return friends;
}

export function useFriends(): readonly Friend[] {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** The friend with this certificate hash, if any. */
export function useFriend(certHash: string | null): Friend | undefined {
  const list = useFriends();
  return certHash === null ? undefined : list.find((f) => f.certHash === certHash);
}

/** Test helper. */
export function resetFriendsStore(): void {
  unsubscribe?.();
  unsubscribe = undefined;
  boundApi = undefined;
  generation += 1;
  friends = [];
}
