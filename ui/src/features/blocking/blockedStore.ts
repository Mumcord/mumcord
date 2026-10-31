/**
 * Blocked users (issue #140), as the engine reports them, and how blocked messages show in
 * channel chat (`settings.blockedMessages`). One store for the app; it binds to the active engine
 * client and re-binds when tests swap it.
 *
 * A block only counts for the own identity in use on a server: by the sender's certificate, or,
 * for a name block (someone without a certificate), by name on the server it was set on.
 */
import { useMemo, useSyncExternalStore } from "react";
import {
  getApi,
  type BlockedMessages,
  type BlockedUser,
  type MumcordApi,
  type Unsubscribe,
} from "../../api";
import { ownCertHash } from "../requests/ownIdentity";
import { getServersState, useServers } from "../servers/store";

let users: readonly BlockedUser[] = [];
let mode: BlockedMessages = "collapse";
let boundApi: MumcordApi | undefined;
let unsubscribe: Unsubscribe | undefined;
let generation = 0;
/** Set once a `blockedUsersChanged` arrived, so a late initial load cannot overwrite it. */
let changedSinceLoad = false;
let modeChanged = false;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  unsubscribe?.();
  boundApi = api;
  users = [];
  mode = "collapse";
  changedSinceLoad = false;
  modeChanged = false;
  const mine = ++generation;
  unsubscribe = api.on("blockedUsersChanged", ({ users: next }) => {
    changedSinceLoad = true;
    users = next;
    notify();
  });
  api.blockedUsers().then(
    (list) => {
      if (generation !== mine || changedSinceLoad) return;
      users = list;
      notify();
    },
    (error: unknown) => {
      console.error("Loading the blocked users failed", error);
    },
  );
  api.settingsGet().then(
    (settings) => {
      if (generation !== mine || modeChanged) return;
      mode = settings.blockedMessages;
      notify();
    },
    (error: unknown) => {
      console.error("Reading the blocked messages setting failed", error);
    },
  );
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

/** Calls `listener` after every change of the blocks or the setting. */
export function subscribeBlocked(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getUsers(): readonly BlockedUser[] {
  ensureBound();
  return users;
}

function getMode(): BlockedMessages {
  ensureBound();
  return mode;
}

/** The blocks of every own identity. */
export function useBlockedUsers(): readonly BlockedUser[] {
  return useSyncExternalStore(subscribeBlocked, getUsers);
}

/** Collapse or hide channel messages of blocked users. */
export function useBlockedMessages(): BlockedMessages {
  return useSyncExternalStore(subscribeBlocked, getMode);
}

/** The settings screen saved a new choice; the engine sends no event for settings. */
export function noteBlockedMessages(next: BlockedMessages): void {
  ensureBound();
  modeChanged = true;
  if (mode === next) return;
  mode = next;
  notify();
}

/**
 * The block of the own identity `own` that covers this person on `serverId`. With `own` unknown
 * (identities not loaded yet) every identity's blocks count, so nothing slips through at start.
 */
export function findBlock(
  list: readonly BlockedUser[],
  own: string | null,
  serverId: number,
  certHash: string | null,
  name: string,
): BlockedUser | undefined {
  const lower = name.toLowerCase();
  return list.find(
    (b) =>
      (own === null || b.ownCertHash === own) &&
      (b.certHash === null
        ? b.serverId === serverId && b.name.toLowerCase() === lower
        : b.certHash === certHash),
  );
}

/** True when the identity used on `serverId` blocked this person. For non-React code. */
export function isBlocked(serverId: number, certHash: string | null, name: string): boolean {
  const list = getUsers();
  if (list.length === 0) return false;
  const { servers, identities } = getServersState();
  const own = ownCertHash(servers, identities, serverId);
  return findBlock(list, own, serverId, certHash, name) !== undefined;
}

/** The block that covers this person on `serverId`, if any. */
export function useBlock(
  serverId: number | null,
  certHash: string | null,
  name: string,
): BlockedUser | undefined {
  const list = useBlockedUsers();
  const { servers, identities } = useServers();
  if (serverId === null || list.length === 0) return undefined;
  return findBlock(list, ownCertHash(servers, identities, serverId), serverId, certHash, name);
}

/**
 * A checker for many people at once (chat lists, the notification centre); it changes whenever
 * the blocks or the identities change.
 */
export function useIsBlocked(): (
  serverId: number,
  certHash: string | null,
  name: string,
) => boolean {
  const list = useBlockedUsers();
  const { servers, identities } = useServers();
  return useMemo(
    () => (serverId: number, certHash: string | null, name: string) =>
      list.length > 0 &&
      findBlock(list, ownCertHash(servers, identities, serverId), serverId, certHash, name) !==
        undefined,
    [list, servers, identities],
  );
}

/** Who a block is for: someone online by session, or anyone by certificate. */
export type BlockHow = { kind: "session"; session: number } | { kind: "cert"; certHash: string };

export function blockPerson(serverId: number, how: BlockHow, name: string): Promise<null> {
  return how.kind === "session"
    ? getApi().blockUser(serverId, how.session)
    : getApi().blockCert(serverId, how.certHash, name);
}

export function unblock(id: number): Promise<null> {
  return getApi().unblock(id);
}

/** Test helper. */
export function resetBlockedStore(): void {
  unsubscribe?.();
  unsubscribe = undefined;
  boundApi = undefined;
  generation += 1;
  users = [];
  mode = "collapse";
}
