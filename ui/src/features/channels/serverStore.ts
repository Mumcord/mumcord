/**
 * Shared live view of the connected server (channel tree, people, talking state). One store for
 * the whole app: every feature reads it through `useServerState()` instead of subscribing to the
 * engine events itself, so the tree, member list, chat and user panel always agree.
 */
import { useMemo, useSyncExternalStore } from "react";
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
import { cachedSelector } from "./cachedSelector";
import { resetCollapseMemory } from "./collapseMemory";

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

/** A state event from the engine, queued until the next frame. */
export type ServerEvent =
  | { kind: "connection"; connection: ConnectionState }
  | { kind: "synced"; snapshot: ServerSnapshot; connection: ConnectionState }
  | { kind: "channelUpserted"; channel: ChannelInfo }
  | { kind: "channelsUpserted"; channels: readonly ChannelInfo[] }
  | { kind: "channelRemoved"; channelId: number }
  | { kind: "userUpserted"; user: User }
  | { kind: "userRemoved"; session: number }
  | { kind: "talk"; session: number; talking: boolean };

/** Runs `flush` soon; returns a function that cancels it. */
export type UpdateScheduler = (flush: () => void) => () => void;

/** Upper bound for an update when no frame comes, e.g. while the window is hidden. */
const FALLBACK_MS = 100;

/**
 * Next animation frame, or `FALLBACK_MS` when the window is hidden and frames pause. A big server
 * sends dozens of events per second (and hundreds right after connecting); applying them once
 * per frame turns each burst into one new state and one render (scale report, P1).
 */
const frameScheduler: UpdateScheduler = (flush) => {
  const frame = requestAnimationFrame(flush);
  const timer = setTimeout(flush, FALLBACK_MS);
  return () => {
    cancelAnimationFrame(frame);
    clearTimeout(timer);
  };
};

// Component tests check the DOM right after an engine event, so they apply events right away;
// the per-frame batching has its own tests (`setUpdateScheduler`).
let scheduler: UpdateScheduler | null = import.meta.env.MODE === "test" ? null : frameScheduler;
let queue: ServerEvent[] = [];
let cancelFlush: (() => void) | null = null;

/**
 * How queued events are applied: per frame (default), with a custom scheduler (tests), or `null`
 * for right away (unit tests that check the state synchronously after an event).
 */
export function setUpdateScheduler(next: UpdateScheduler | null): void {
  flushServerUpdates();
  scheduler = next;
}

function enqueue(event: ServerEvent): void {
  queue.push(event);
  if (scheduler === null) {
    flushServerUpdates();
  } else if (cancelFlush === null) {
    cancelFlush = scheduler(flushServerUpdates);
  }
}

/** Applies all queued events now: one new state, one notification. */
export function flushServerUpdates(): void {
  cancelFlush?.();
  cancelFlush = null;
  if (queue.length === 0) return;
  const events = queue;
  queue = [];
  const { next, changes } = applyEvents(state, events);
  if (next === state && changes.length === 0) return;
  const connected = next.serverId !== null && next.connection.state === "connected";
  state = next.connected === connected ? next : { ...next, connected };
  for (const listener of listeners) listener();
  for (const change of changes) {
    for (const listener of changeListeners) listener(change, state);
  }
}

/**
 * Applies a batch of events to `base`. The maps are copied at most once per batch, and
 * everything that did not change keeps its identity, so memoised views of unchanged channels
 * and people skip re-rendering. Change listeners get the changes in order; runs of `channels`
 * or `connection` changes collapse into one, since their listeners only look at the new state.
 */
export function applyEvents(
  base: ServerState,
  events: readonly ServerEvent[],
): { next: ServerState; changes: ServerChange[] } {
  let next = base;
  let channels: Map<number, ChannelInfo> | null = null;
  let users: Map<number, User> | null = null;
  let talking: Set<number> | null = null;
  const changes: ServerChange[] = [];
  const replace = (whole: ServerState) => {
    next = whole;
    channels = null;
    users = null;
    talking = null;
  };
  const ownChannels = () => {
    if (channels === null) {
      channels = new Map(next.channels);
      next = { ...next, channels };
    }
    return channels;
  };
  const ownUsers = () => {
    if (users === null) {
      users = new Map(next.users);
      next = { ...next, users };
    }
    return users;
  };
  const ownTalking = () => {
    if (talking === null) {
      talking = new Set(next.talking);
      next = { ...next, talking };
    }
    return talking;
  };
  const record = (change: ServerChange) => {
    const last = changes[changes.length - 1];
    if (
      last?.kind === change.kind &&
      (change.kind === "channels" || change.kind === "connection")
    ) {
      return;
    }
    changes.push(change);
  };
  for (const event of events) {
    switch (event.kind) {
      case "connection":
        // Reconnecting keeps the last tree visible; only a real disconnect clears it.
        if (event.connection.state === "disconnected" || event.connection.state === "connecting") {
          replace({ ...EMPTY, connection: event.connection });
        } else {
          next = { ...next, connection: event.connection };
        }
        record({ kind: "connection" });
        break;
      case "synced":
        replace(fromSnapshot(event.snapshot, event.connection));
        record({ kind: "synced" });
        break;
      case "channelUpserted":
        ownChannels().set(event.channel.id, event.channel);
        record({ kind: "channels" });
        break;
      case "channelsUpserted": {
        const map = ownChannels();
        for (const channel of event.channels) map.set(channel.id, channel);
        record({ kind: "channels" });
        break;
      }
      case "channelRemoved":
        ownChannels().delete(event.channelId);
        record({ kind: "channels" });
        break;
      case "userUpserted": {
        const previous = next.users.get(event.user.session);
        ownUsers().set(event.user.session, event.user);
        record({ kind: "userUpserted", previous, user: event.user });
        break;
      }
      case "userRemoved": {
        const user = next.users.get(event.session);
        if (!user) break;
        ownUsers().delete(event.session);
        if (next.talking.has(event.session)) ownTalking().delete(event.session);
        record({ kind: "userRemoved", user });
        break;
      }
      case "talk":
        if (next.talking.has(event.session) === event.talking) break;
        if (event.talking) ownTalking().add(event.session);
        else ownTalking().delete(event.session);
        record({ kind: "talk", session: event.session, talking: event.talking });
        break;
    }
  }
  return { next, changes };
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
  queue = [];
  cancelFlush?.();
  cancelFlush = null;
  unsubscribers = [
    api.on("connectionStateChanged", ({ state: connection }) => {
      enqueue({ kind: "connection", connection });
    }),
    api.on("serverSynced", ({ snapshot }) => {
      const connection: ConnectionState = { state: "connected", serverId: snapshot.serverId };
      enqueue({ kind: "synced", snapshot, connection });
    }),
    api.on("channelUpserted", ({ channel }) => {
      enqueue({ kind: "channelUpserted", channel });
    }),
    api.on("channelsUpserted", ({ channels }) => {
      enqueue({ kind: "channelsUpserted", channels });
    }),
    api.on("channelRemoved", ({ channelId }) => {
      enqueue({ kind: "channelRemoved", channelId });
    }),
    api.on("userUpserted", ({ user }) => {
      enqueue({ kind: "userUpserted", user });
    }),
    api.on("userRemoved", ({ session }) => {
      enqueue({ kind: "userRemoved", session });
    }),
    api.on("talkStateChanged", ({ session, talking }) => {
      enqueue({ kind: "talk", session, talking });
    }),
  ];
  // Events registered above win over this initial read if they arrive first.
  void Promise.all([api.connectionState(), api.sessionSnapshot()])
    .then(([connection, snapshot]) => {
      flushServerUpdates();
      if (boundApi !== api || state.serverId !== null) return;
      if (snapshot) enqueue({ kind: "synced", snapshot, connection });
      else enqueue({ kind: "connection", connection });
      flushServerUpdates();
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

/** Current server state; re-renders on every change (at most once per frame). */
export function useServerState(): ServerState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/**
 * One value derived from the server state; re-renders only when it changes according to `equal`
 * (`Object.is` by default, so return primitives or values that keep their identity).
 */
export function useServerSelector<T>(
  selector: (state: ServerState) => T,
  equal: (a: T, b: T) => boolean = Object.is,
): T {
  const getSelection = useMemo(
    () => cachedSelector(getSnapshot, selector, equal),
    [selector, equal],
  );
  return useSyncExternalStore(subscribe, getSelection);
}

/** Non-React read of the current state, including events still waiting for the next frame. */
export function getServerState(): ServerState {
  ensureBound();
  flushServerUpdates();
  return state;
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

/**
 * Test helper: drops the binding so the next use binds to the then-active client, and forgets the
 * remembered expand/collapse choices of the old session.
 */
export function resetServerStore(): void {
  resetCollapseMemory();
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  changeListeners.clear();
  boundApi = undefined;
  state = EMPTY;
  queue = [];
  cancelFlush?.();
  cancelFlush = null;
}
