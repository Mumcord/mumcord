/**
 * Shared live view of the connected servers (channel tree, people, talking state), one state per
 * server id (FR-2.1). One store for the whole app: every feature reads it here instead of
 * subscribing to the engine events itself, so the tree, member list, chat and user panel agree.
 *
 * Two servers are special: the **shown server**, whose channels, chat and members the main window
 * displays (FR-2.5), and the **voice server**, the one server with voice (FR-2.2). They can
 * differ. `useServerState()` and friends describe the shown server; `useServerStateOf(id)` and
 * `useServers()` any connected one.
 */
import { useCallback, useMemo, useSyncExternalStore } from "react";
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
  /** Channels received so far while the first sync is still running (FR-2.24); 0 otherwise. */
  syncChannels: number;
}

/** Changes the store reports besides the new state, e.g. for join/leave announcements. */
export type ServerChange =
  /** Another server is shown now (only reported to `subscribeServerChanges`). */
  | { kind: "shown" }
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
  syncChannels: 0,
};

/** Connected servers (any state but `disconnected`), by server id. */
let servers: ReadonlyMap<number, ServerState> = new Map();
let shownServer: number | null = null;
let voiceServer: number | null = null;
/** Set once the voice server is known, so the initial query can't overwrite a newer event. */
let voiceSeen = false;
/** State of the shown server; after its disconnect, the final state with the error, if any. */
let state: ServerState = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();
const changeListeners = new Set<(change: ServerChange, state: ServerState) => void>();
const allChangeListeners = new Set<
  (serverId: number, change: ServerChange, state: ServerState) => void
>();

/** A state event from the engine, queued until the next frame. */
export type ServerEvent =
  | { kind: "connection"; connection: ConnectionState }
  | { kind: "synced"; snapshot: ServerSnapshot; connection: ConnectionState }
  | { kind: "syncProgress"; channels: number }
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
let queue: { serverId: number; event: ServerEvent }[] = [];
let cancelFlush: (() => void) | null = null;

/**
 * How queued events are applied: per frame (default), with a custom scheduler (tests), or `null`
 * for right away (unit tests that check the state synchronously after an event).
 */
export function setUpdateScheduler(next: UpdateScheduler | null): void {
  flushServerUpdates();
  scheduler = next;
}

function enqueue(serverId: number, event: ServerEvent): void {
  queue.push({ serverId, event });
  if (scheduler === null) {
    flushServerUpdates();
  } else if (cancelFlush === null) {
    cancelFlush = scheduler(flushServerUpdates);
  }
}

/** Applies all queued events now: one new state per server, one notification. */
export function flushServerUpdates(): void {
  cancelFlush?.();
  cancelFlush = null;
  if (queue.length === 0) return;
  const byServer = new Map<number, ServerEvent[]>();
  for (const { serverId, event } of queue) {
    const events = byServer.get(serverId);
    if (events) events.push(event);
    else byServer.set(serverId, [event]);
  }
  queue = [];
  const nextServers = new Map(servers);
  const reports: { serverId: number; changes: ServerChange[]; state: ServerState }[] = [];
  for (const [serverId, events] of byServer) {
    const base = servers.get(serverId) ?? (shownServer === serverId ? state : EMPTY);
    const applied = applyEvents(base, events);
    if (applied.next === base && applied.changes.length === 0) continue;
    const connected =
      applied.next.serverId !== null && applied.next.connection.state === "connected";
    const next =
      applied.next.connected === connected ? applied.next : { ...applied.next, connected };
    // A disconnect clears only this server's state (FR-2.8).
    if (next.connection.state === "disconnected") nextServers.delete(serverId);
    else nextServers.set(serverId, next);
    reports.push({ serverId, changes: applied.changes, state: next });
  }
  if (reports.length === 0) return;
  servers = nextServers;
  const previousShown = shownServer;
  const shownReport = reports.find((report) => report.serverId === shownServer);
  if (shownServer === null || shownReport?.state.connection.state === "disconnected") {
    shownServer = fallbackServer();
  }
  // Without any connected server, the last shown one's final state (and error) stays visible.
  state =
    (shownServer === null ? undefined : servers.get(shownServer)) ?? shownReport?.state ?? state;
  for (const listener of listeners) listener();
  if (shownServer !== null && shownServer !== previousShown) reportShown();
  // When the last server disconnects, the shown server's own changes report it.
  const reported = shownServer ?? previousShown;
  for (const report of reports) {
    for (const change of report.changes) {
      for (const listener of allChangeListeners) listener(report.serverId, change, report.state);
      if (report.serverId !== reported) continue;
      for (const listener of changeListeners) listener(change, state);
    }
  }
}

/** The server to show when none is: the voice server, else the first connected one. */
function fallbackServer(): number | null {
  if (voiceServer !== null && servers.has(voiceServer)) return voiceServer;
  return servers.keys().next().value ?? null;
}

function reportShown(): void {
  for (const listener of changeListeners) listener({ kind: "shown" }, state);
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
      case "syncProgress":
        next = { ...next, syncChannels: event.channels };
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
    syncChannels: 0,
  };
}

function clear(): void {
  servers = new Map();
  shownServer = null;
  voiceServer = null;
  voiceSeen = false;
  state = EMPTY;
  queue = [];
  cancelFlush?.();
  cancelFlush = null;
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  clear();
  unsubscribers = [
    api.on("connectionStateChanged", ({ serverId, state: connection }) => {
      enqueue(serverId, { kind: "connection", connection });
    }),
    api.on("serverSynced", ({ snapshot }) => {
      const connection: ConnectionState = { state: "connected", serverId: snapshot.serverId };
      enqueue(snapshot.serverId, { kind: "synced", snapshot, connection });
    }),
    api.on("syncProgress", ({ serverId, channels }) => {
      enqueue(serverId, { kind: "syncProgress", channels });
    }),
    api.on("channelUpserted", ({ serverId, channel }) => {
      enqueue(serverId, { kind: "channelUpserted", channel });
    }),
    api.on("channelsUpserted", ({ serverId, channels }) => {
      enqueue(serverId, { kind: "channelsUpserted", channels });
    }),
    api.on("channelRemoved", ({ serverId, channelId }) => {
      enqueue(serverId, { kind: "channelRemoved", channelId });
    }),
    api.on("userUpserted", ({ serverId, user }) => {
      enqueue(serverId, { kind: "userUpserted", user });
    }),
    api.on("userRemoved", ({ serverId, session }) => {
      enqueue(serverId, { kind: "userRemoved", session });
    }),
    api.on("talkStateChanged", ({ serverId, session, talking }) => {
      enqueue(serverId, { kind: "talk", session, talking });
    }),
    api.on("voiceServerChanged", ({ serverId }) => {
      setVoiceServer(serverId);
    }),
  ];
  // Events registered above win over this initial read if they arrive first.
  void api
    .connectionsList()
    .then(async (list) => {
      const snapshots = await Promise.all(list.map((entry) => api.sessionSnapshot(entry.serverId)));
      flushServerUpdates();
      if (boundApi !== api) return;
      list.forEach((entry, index) => {
        if ((servers.get(entry.serverId)?.serverId ?? null) !== null) return;
        const snapshot = snapshots[index];
        if (snapshot)
          enqueue(entry.serverId, { kind: "synced", snapshot, connection: entry.state });
        else enqueue(entry.serverId, { kind: "connection", connection: entry.state });
      });
      flushServerUpdates();
    })
    .catch((error: unknown) => {
      console.error("Loading the server state failed", error);
    });
  void api.voiceServerGet().then(
    (serverId) => {
      if (boundApi === api && !voiceSeen) setVoiceServer(serverId);
    },
    (error: unknown) => {
      console.error("Loading the voice server failed", error);
    },
  );
}

function setVoiceServer(serverId: number | null): void {
  voiceSeen = true;
  if (voiceServer === serverId) return;
  voiceServer = serverId;
  for (const listener of listeners) listener();
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

/** State of the shown server; re-renders on every change (at most once per frame). */
export function useServerState(): ServerState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/**
 * One value derived from the shown server's state; re-renders only when it changes according to
 * `equal` (`Object.is` by default, so return primitives or values that keep their identity).
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

function getServers(): ReadonlyMap<number, ServerState> {
  ensureBound();
  return servers;
}

/** All connected servers (reconnecting and connecting ones included), by server id. */
export function useServers(): ReadonlyMap<number, ServerState> {
  return useSyncExternalStore(subscribe, getServers);
}

/** State of one server; empty while it is not connected. */
export function useServerStateOf(serverId: number | null): ServerState {
  const get = useCallback(
    () => (serverId === null ? undefined : getServers().get(serverId)) ?? EMPTY,
    [serverId],
  );
  return useSyncExternalStore(subscribe, get);
}

function getShown(): number | null {
  ensureBound();
  return shownServer;
}

function getVoice(): number | null {
  ensureBound();
  return voiceServer;
}

/** The server whose channels, chat and members are shown (FR-2.5); `null` when none is. */
export function useShownServerId(): number | null {
  return useSyncExternalStore(subscribe, getShown);
}

/** The server with voice (FR-2.2); `null` while every server is read-only. */
export function useVoiceServerId(): number | null {
  return useSyncExternalStore(subscribe, getVoice);
}

/** Non-React read of the shown server; `null` when none is. */
export function getShownServerId(): number | null {
  flushServerUpdates();
  return getShown();
}

/** Non-React read of the voice server; `null` while every server is read-only. */
export function getVoiceServerId(): number | null {
  return getVoice();
}

/**
 * Shows `serverId`'s channels, chat and members (FR-2.5); voice stays where it is. A server that
 * is not connected yet shows as empty until it connects.
 */
export function showServer(serverId: number): void {
  flushServerUpdates();
  if (getShown() === serverId) return;
  shownServer = serverId;
  state = servers.get(serverId) ?? EMPTY;
  for (const listener of listeners) listener();
  reportShown();
}

/** Never a saved server's id: commands for it fail with `notConnected`. */
const NO_SERVER = 0;

/**
 * Server that commands about what the window shows go to (chat, people, channels). Read during
 * render too, so it does not apply queued events: that would update other components mid-render.
 */
export function shownServerId(): number {
  return getShown() ?? NO_SERVER;
}

/** Server that voice commands go to; without one, they fail with `notConnected`. */
export function voiceServerId(): number {
  return getVoiceServerId() ?? NO_SERVER;
}

/** Non-React read of the shown server's state, including events still waiting for the next frame. */
export function getServerState(): ServerState {
  ensureBound();
  flushServerUpdates();
  return state;
}

/** Non-React read of one server's state; empty while it is not connected. */
export function getServerStateOf(serverId: number): ServerState {
  ensureBound();
  flushServerUpdates();
  return servers.get(serverId) ?? EMPTY;
}

/**
 * Listens to semantic changes (joins, leaves, talking) of the shown server, and to `shown` when
 * another server is shown; returns an unsubscribe function.
 */
export function subscribeServerChanges(
  listener: (change: ServerChange, state: ServerState) => void,
): () => void {
  ensureBound();
  changeListeners.add(listener);
  return () => {
    changeListeners.delete(listener);
  };
}

/** Listens to semantic changes of every connected server; returns an unsubscribe function. */
export function subscribeAllServerChanges(
  listener: (serverId: number, change: ServerChange, state: ServerState) => void,
): () => void {
  ensureBound();
  allChangeListeners.add(listener);
  return () => {
    allChangeListeners.delete(listener);
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
  allChangeListeners.clear();
  boundApi = undefined;
  clear();
}
