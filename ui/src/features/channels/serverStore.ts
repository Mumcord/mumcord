/**
 * Shared live view of the connected server (channel tree, people, talking state). One store for
 * the whole app: every feature reads it through `useServerState()` instead of subscribing to the
 * engine events itself, so the tree, member list, chat and user panel always agree.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type ChannelInfo,
  type ConnectionState,
  type MumcordApi,
  type ServerInfo,
  type ServerSnapshot,
  type Unsubscribe,
  type User,
} from "../../api";

export interface ServerState {
  connection: ConnectionState;
  /** True while connected and synced (tree, people and server info are available). */
  connected: boolean;
  /** `null` until the first `serverSynced` of the current connection. */
  serverId: number | null;
  selfSession: number | null;
  info: ServerInfo | null;
  channels: ReadonlyMap<number, ChannelInfo>;
  users: ReadonlyMap<number, User>;
  /** Sessions currently talking (own user included). */
  talking: ReadonlySet<number>;
  /** The engine's read-only state (fresh connect, QA.2) at the last sync. */
  readOnly: boolean;
  /** Channel offered for rejoining at the last sync: the app last quit while in it (QA.9). */
  rejoinChannelId: number | null;
}

/** Changes the store reports besides the new state, e.g. for join/leave announcements. */
export type ServerChange =
  | { kind: "synced" }
  | { kind: "connection" }
  | { kind: "channels" }
  | { kind: "userUpserted"; previous: User | undefined; user: User }
  | { kind: "userRemoved"; user: User }
  | { kind: "talk"; session: number; talking: boolean };

const EMPTY: ServerState = {
  connection: { state: "disconnected", error: null },
  connected: false,
  serverId: null,
  selfSession: null,
  info: null,
  channels: new Map(),
  users: new Map(),
  talking: new Set(),
  readOnly: false,
  rejoinChannelId: null,
};

let state: ServerState = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();
const changeListeners = new Set<(change: ServerChange, state: ServerState) => void>();

function set(next: ServerState, change?: ServerChange): void {
  const connected = next.serverId !== null && next.connection.state === "connected";
  state = next.connected === connected ? next : { ...next, connected };
  for (const listener of listeners) listener();
  if (change) for (const listener of changeListeners) listener(change, state);
}

function fromSnapshot(snapshot: ServerSnapshot, connection: ConnectionState): ServerState {
  return {
    connection,
    connected: connection.state === "connected",
    serverId: snapshot.serverId,
    selfSession: snapshot.selfSession,
    info: snapshot.info,
    channels: new Map(snapshot.channels.map((c) => [c.id, c])),
    users: new Map(snapshot.users.map((u) => [u.session, u])),
    talking: new Set(),
    readOnly: snapshot.readOnly,
    rejoinChannelId: snapshot.rejoinChannelId,
  };
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  state = EMPTY;
  unsubscribers = [
    api.on("connectionStateChanged", ({ state: connection }) => {
      // Reconnecting keeps the last tree visible; only a real disconnect clears it.
      if (connection.state === "disconnected" || connection.state === "connecting") {
        set({ ...EMPTY, connection }, { kind: "connection" });
      } else {
        set({ ...state, connection }, { kind: "connection" });
      }
    }),
    api.on("serverSynced", ({ snapshot }) => {
      const connection: ConnectionState = { state: "connected", serverId: snapshot.serverId };
      set(fromSnapshot(snapshot, connection), { kind: "synced" });
    }),
    api.on("channelUpserted", ({ channel }) => {
      const channels = new Map(state.channels);
      channels.set(channel.id, channel);
      set({ ...state, channels }, { kind: "channels" });
    }),
    api.on("channelRemoved", ({ channelId }) => {
      const channels = new Map(state.channels);
      channels.delete(channelId);
      set({ ...state, channels }, { kind: "channels" });
    }),
    api.on("userUpserted", ({ user }) => {
      const previous = state.users.get(user.session);
      const users = new Map(state.users);
      users.set(user.session, user);
      set({ ...state, users }, { kind: "userUpserted", previous, user });
    }),
    api.on("userRemoved", ({ session }) => {
      const user = state.users.get(session);
      if (!user) return;
      const users = new Map(state.users);
      users.delete(session);
      const talking = new Set(state.talking);
      talking.delete(session);
      set({ ...state, users, talking }, { kind: "userRemoved", user });
    }),
    api.on("talkStateChanged", ({ session, talking: isTalking }) => {
      if (state.talking.has(session) === isTalking) return;
      const talking = new Set(state.talking);
      if (isTalking) talking.add(session);
      else talking.delete(session);
      set({ ...state, talking }, { kind: "talk", session, talking: isTalking });
    }),
  ];
  // Events registered above win over this initial read if they arrive first.
  void Promise.all([api.connectionState(), api.sessionSnapshot()])
    .then(([connection, snapshot]) => {
      if (boundApi !== api || state.serverId !== null) return;
      if (snapshot) set(fromSnapshot(snapshot, connection), { kind: "synced" });
      else set({ ...state, connection }, { kind: "connection" });
    })
    .catch((error: unknown) => {
      console.error("Loading the server state failed", error);
    });
}

/** Binds to the active engine client; re-binds when tests swap it with `setApi()`. */
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

function getSnapshot(): ServerState {
  ensureBound();
  return state;
}

/** Current server state; re-renders on every change. */
export function useServerState(): ServerState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** Non-React read of the current state. */
export function getServerState(): ServerState {
  return getSnapshot();
}

/** Listens to semantic changes (joins, leaves, talking); returns an unsubscribe function. */
export function subscribeServerChanges(
  listener: (change: ServerChange, state: ServerState) => void,
): () => void {
  ensureBound();
  changeListeners.add(listener);
  return () => {
    changeListeners.delete(listener);
  };
}

/** The own user, if connected and synced. */
export function selfUser(s: ServerState): User | undefined {
  return s.selfSession === null ? undefined : s.users.get(s.selfSession);
}

/** Test helper: drops the binding so the next use binds to the then-active client. */
export function resetServerStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  changeListeners.clear();
  boundApi = undefined;
  state = EMPTY;
}
