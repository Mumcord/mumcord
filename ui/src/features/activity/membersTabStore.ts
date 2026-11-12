/**
 * Which tab of the members column is open (FR-11.2): remembered per server for the session, and
 * back to "Activity" at the first sync of a new connection, so every connection opens on what is
 * happening on the server (issue #709).
 */
import { useCallback, useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import { subscribeAllServerChanges } from "../channels/serverStore";

export type MembersTab = "members" | "activity";

/** The tab every connection starts on. */
const DEFAULT_TAB: MembersTab = "activity";

let choices = new Map<number | null, MembersTab>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  choices = new Map();
  unsubscribers = [
    subscribeAllServerChanges((serverId, change) => {
      if (change.kind !== "synced" || !choices.has(serverId)) return;
      choices.delete(serverId);
      notify();
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

export function membersTabOf(serverId: number | null): MembersTab {
  ensureBound();
  return choices.get(serverId) ?? DEFAULT_TAB;
}

export function selectMembersTab(serverId: number | null, tab: MembersTab): void {
  ensureBound();
  if (membersTabOf(serverId) === tab) return;
  choices.set(serverId, tab);
  notify();
}

export function useMembersTab(serverId: number | null): MembersTab {
  const get = useCallback(() => membersTabOf(serverId), [serverId]);
  return useSyncExternalStore(subscribe, get);
}

/** Test helper: forgets the binding and the remembered choices. */
export function resetMembersTabStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  listeners.clear();
  choices = new Map();
}
