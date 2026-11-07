/**
 * The activity log (FR-11.4 – FR-11.8): what happened on each server since this client connected.
 * Plain data module, no rendering (that is the feed in T5): it follows the stores that already
 * hold the information, turns their changes into per-server events and hands them out newest first.
 *
 * Capped at `MAX_EVENTS` per server and saved on this device only (#594, `activityHistory.ts`), so
 * the feed survives a reconnect or a restart; events from before the current connection are marked
 * `earlier` and never count as live. What `serverSynced` delivers is the baseline (FR-11.5): the log
 * keeps what it last saw per source and only diffs later changes, so a share, party or activity
 * that was already running when we connected is never an event.
 */
import { useCallback, useSyncExternalStore } from "react";
import {
  getApi,
  type MumcordApi,
  type PluginActivityEvent,
  type StageActivityView,
  type Unsubscribe,
} from "../../api";
import {
  getServerStateOf,
  getVoiceServerId,
  subscribeAllServerChanges,
  type ServerChange,
  type ServerState,
} from "../channels/serverStore";
import {
  activitiesLoaded,
  stageActivities,
  subscribeStageActivities,
} from "../plugins/stage/activityStore";
import { sharesOf, subscribeShares } from "../screenshare/sharesStore";
import { subscribeWatchParty, watchPartyOf } from "../watchparty/watchPartyStore";
import { clearSavedHistory, loadHistory, saveHistory } from "./activityHistory";

/** Cap per server (FR-11.8); the 201st event drops the oldest. */
export const MAX_EVENTS = 200;

/** What a live row points at, so the feed can tell a still-running thing from an ended one (FR-11.12). */
export type LiveReference =
  /** `sharesOf(serverId)` still holds that session. */
  | { kind: "screenShare"; session: number }
  /** A server has at most one party, so the event's server id is the reference. */
  | { kind: "watchParty" }
  /** The id the host gave the activity, which `stageJoin` takes; still in `stageActivities()`. */
  | { kind: "pluginActivity"; id: string };

/** What a plugin pushed with `activity.push` (FR-11.9); its action goes back through `ui-action` (FR-11.13). */
export interface PluginEventInfo {
  pluginId: string;
  text: string;
  actionLabel: string | null;
  actionId: string | null;
}

/** Every event carries its server and the local time it was observed (FR-11.7). */
interface EventBase {
  serverId: number;
  /** `Date.now()` milliseconds; the feed groups the rows by day from it (FR-11.10). */
  at: number;
  /**
   * Set on events from before the current connection (#594): saved ones after a restart and the
   * previous connection's after a reconnect. Their live references may point at anything now, so
   * they are always shown as ended.
   */
  earlier?: true;
}

/**
 * One event of the feed (FR-11.6). `name` is the actor's display name as it was known at event
 * time, `null` when the person is not in the state any more, and `self` marks the user's own
 * session so the text is localized as "You …" (T6).
 */
export type ActivityEvent =
  | (EventBase & { kind: "connected" | "disconnected"; name: string | null; self: boolean })
  | (EventBase & { kind: "moved"; name: string | null; self: boolean; channelName: string | null })
  | (EventBase & {
      kind: "screenShare" | "watchParty";
      name: string | null;
      self: boolean;
      /** `true` for "started", `false` for "stopped". */
      started: boolean;
      channelName: string | null;
      live: LiveReference;
    })
  | (EventBase & {
      kind: "pluginActivity";
      name: string | null;
      self: boolean;
      started: boolean;
      /** Held with the event: the stage's activity object is gone once it stops. */
      pluginName: string;
      channelName: string | null;
      live: LiveReference;
    })
  | (EventBase & { kind: "pluginEvent"; plugin: PluginEventInfo });

/**
 * What the log last saw on one server. The first version is the connect-time state and produces no
 * event; every change after it is diffed against it. A source's entries are remembered because the
 * object itself is gone when it stops: a stopped share, an ended party, a left activity.
 */
interface Baseline {
  /** Sharing sessions, with the display name last known for them. */
  shares: Map<number, string | null>;
  /** The party's host session, `null` when no party runs. */
  party: number | null;
  /** Activities by id, as the host listed them. */
  activities: Map<string, StageActivityView>;
  /** Per source: whether its connect-time state has landed. Nothing is an event before that. */
  ready: { shares: boolean; party: boolean; stage: boolean };
}

const EMPTY_LOG: readonly ActivityEvent[] = [];

let logs = new Map<number, readonly ActivityEvent[]>();
let baselines = new Map<number, Baseline>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/** The server's log, loaded from the saved history the first time it is needed. */
function current(serverId: number): readonly ActivityEvent[] {
  let log = logs.get(serverId);
  if (log === undefined) {
    log = loadHistory(serverId);
    logs.set(serverId, log);
  }
  return log;
}

function store(serverId: number, log: readonly ActivityEvent[]): void {
  logs.set(serverId, log);
  saveHistory(serverId, log);
  notify();
}

/** Newest first, so the feed renders the list as it gets it (FR-11.10, FR-11.8). */
function add(serverId: number, event: ActivityEvent): void {
  const next = [event, ...current(serverId)];
  if (next.length > MAX_EVENTS) next.length = MAX_EVENTS;
  store(serverId, next);
}

/** The end of a connection ends the diffing; its events stay in the log and in the history. */
function forgetBaseline(serverId: number): void {
  baselines.delete(serverId);
}

function nameOf(state: ServerState, session: number | null): string | null {
  return session === null ? null : (state.users.get(session)?.name ?? null);
}

function channelNameOf(state: ServerState, channelId: number | null): string | null {
  return channelId === null ? null : (state.channels.get(channelId)?.name ?? null);
}

/** The user's own channel on `state`; `null` while the own session is not in the state. */
function ownChannelId(state: ServerState): number | null {
  const self = state.selfSession === null ? undefined : state.users.get(state.selfSession);
  return self?.channelId ?? null;
}

/** The channel a live row shows: shares, parties and activities are all in the user's voice channel. */
function voiceChannelName(state: ServerState): string | null {
  return channelNameOf(state, ownChannelId(state));
}

function isSelf(state: ServerState, session: number | null): boolean {
  return session !== null && session === state.selfSession;
}

function record(
  serverId: number,
  kind: "screenShare" | "watchParty",
  name: string | null,
  self: boolean,
  channelName: string | null,
  started: boolean,
  live: LiveReference,
): void {
  add(serverId, { serverId, at: Date.now(), kind, name, self, channelName, started, live });
}

/**
 * The connect-time state per source, read from the engine instead of from the UI stores: those
 * reset themselves at `serverSynced` and fill up again afterwards, so a delivery inside that window
 * cannot be told from a real change, and a share or party already running at connect would read as
 * "started". Deliverments before the baseline lands are not events; the one after it is diffed.
 */
function captureBaseline(serverId: number): void {
  const baseline: Baseline = {
    shares: new Map(),
    party: null,
    activities: new Map(),
    // The stage keeps one global list, so an answer already received is the state at connect.
    ready: { shares: false, party: false, stage: activitiesLoaded() },
  };
  baselines.set(serverId, baseline);
  if (baseline.ready.stage) rememberActivities(baseline, stageActivities());
  const api = getApi();
  void Promise.all([api.screenShareList(serverId), api.watchPartyGet(serverId)]).then(
    ([shares, party]) => {
      // A newer sync of the same server replaced this baseline; its own diff is the one to run.
      if (baselines.get(serverId) !== baseline) return;
      markBaseline(
        serverId,
        baseline,
        shares.map((share) => share.session),
        party === null ? null : party.host,
      );
    },
    (error: unknown) => {
      if (baselines.get(serverId) !== baseline) return;
      console.error("Loading the activity baseline failed", error);
      // Without the engine's answer the stores' current lists are the best baseline available.
      markBaseline(
        serverId,
        baseline,
        sharesOf(serverId).map((share) => share.session),
        watchPartyOf(serverId)?.party.host ?? null,
      );
    },
  );
}

/** Names the sessions the baseline holds, so a stopped row can still name its person. */
function markBaseline(
  serverId: number,
  baseline: Baseline,
  shareSessions: readonly number[],
  partyHost: number | null,
): void {
  const state = getServerStateOf(serverId);
  for (const session of shareSessions) baseline.shares.set(session, nameOf(state, session));
  baseline.party = partyHost;
  baseline.ready.shares = true;
  baseline.ready.party = true;
}

function diffShares(serverId: number, baseline: Baseline): void {
  if (!baseline.ready.shares) return;
  const state = getServerStateOf(serverId);
  const now = new Map<number, string | null>();
  for (const share of sharesOf(serverId)) now.set(share.session, nameOf(state, share.session));
  for (const [session, name] of now) {
    if (baseline.shares.has(session)) continue;
    record(serverId, "screenShare", name, isSelf(state, session), voiceChannelName(state), true, {
      kind: "screenShare",
      session,
    });
  }
  for (const [session, remembered] of baseline.shares) {
    if (now.has(session)) continue;
    record(
      serverId,
      "screenShare",
      nameOf(state, session) ?? remembered,
      isSelf(state, session),
      voiceChannelName(state),
      false,
      { kind: "screenShare", session },
    );
  }
  baseline.shares = now;
}

function diffParty(serverId: number, baseline: Baseline): void {
  if (!baseline.ready.party) return;
  const state = getServerStateOf(serverId);
  const live = watchPartyOf(serverId);
  const host = live === null ? null : live.party.host;
  if (host === baseline.party) return;
  const started = host !== null;
  // The party object is gone once it ends, so the stopped row names the host it had before.
  const person = host ?? baseline.party;
  record(
    serverId,
    "watchParty",
    nameOf(state, person),
    isSelf(state, person),
    live === null ? voiceChannelName(state) : channelNameOf(state, live.party.channelId),
    started,
    { kind: "watchParty" },
  );
  baseline.party = host;
}

/**
 * The plugin stage has one global list for the user's channel on the server the plugins see, which
 * is the voice server (FR-9.46), so its rows belong to that server.
 */
function diffStage(): void {
  const serverId = getVoiceServerId();
  if (serverId === null) return;
  const baseline = baselines.get(serverId);
  if (baseline === undefined) return;
  if (!baseline.ready.stage) {
    // The host's first answer is the state at connect, not a change (FR-11.5).
    baseline.ready.stage = true;
    rememberActivities(baseline, stageActivities());
    return;
  }
  const state = getServerStateOf(serverId);
  const now = new Map<string, StageActivityView>();
  for (const activity of stageActivities()) {
    // A "Get plugin" notice is not an activity the log records (FR-11.6).
    if (activity.kind === "activity") now.set(activity.id, activity);
  }
  for (const [id, activity] of now) {
    if (baseline.activities.has(id)) continue;
    recordStage(serverId, activity, true, state);
  }
  for (const [id, remembered] of baseline.activities) {
    if (now.has(id)) continue;
    recordStage(serverId, remembered, false, state);
  }
  baseline.activities = now;
}

function recordStage(
  serverId: number,
  activity: StageActivityView,
  started: boolean,
  state: ServerState,
): void {
  add(serverId, {
    serverId,
    at: Date.now(),
    kind: "pluginActivity",
    name: nameOf(state, activity.starter) ?? activity.starterName,
    self: isSelf(state, activity.starter),
    // The feed shows the plugin's name even after its activity left the stage (FR-11.11).
    pluginName: activity.pluginName || activity.plugin,
    channelName: voiceChannelName(state),
    started,
    live: { kind: "pluginActivity", id: activity.id },
  });
}

function rememberActivities(baseline: Baseline, list: readonly StageActivityView[]): void {
  baseline.activities = new Map(
    list
      .filter((activity) => activity.kind === "activity")
      .map((activity) => [activity.id, activity]),
  );
}

function onServerChange(serverId: number, change: ServerChange, state: ServerState): void {
  switch (change.kind) {
    case "synced": {
      // FR-11.5: the snapshot is the baseline; only the start of the session is recorded. What
      // the log already holds is from before this connection (#594).
      logs.set(
        serverId,
        current(serverId).map((event) =>
          event.earlier === true ? event : { ...event, earlier: true },
        ),
      );
      add(serverId, {
        serverId,
        at: Date.now(),
        kind: "connected",
        name: nameOf(state, state.selfSession),
        self: true,
      });
      captureBaseline(serverId);
      break;
    }
    case "userUpserted": {
      if (change.previous === undefined) {
        // The own session is the "You connected" entry of this connection.
        if (isSelf(state, change.user.session)) break;
        add(serverId, {
          serverId,
          at: Date.now(),
          kind: "connected",
          name: change.user.name,
          self: false,
        });
      } else if (change.previous.channelId !== change.user.channelId) {
        add(serverId, {
          serverId,
          at: Date.now(),
          kind: "moved",
          name: change.user.name,
          self: isSelf(state, change.user.session),
          channelName: channelNameOf(state, change.user.channelId),
        });
      }
      break;
    }
    case "userRemoved": {
      add(serverId, {
        serverId,
        at: Date.now(),
        kind: "disconnected",
        name: change.user.name,
        self: isSelf(state, change.user.session),
      });
      break;
    }
  }
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  logs = new Map();
  baselines = new Map();
  unsubscribers = [
    subscribeAllServerChanges(onServerChange),
    subscribeShares(() => {
      for (const [serverId, baseline] of baselines) diffShares(serverId, baseline);
    }),
    subscribeWatchParty(() => {
      for (const [serverId, baseline] of baselines) diffParty(serverId, baseline);
    }),
    subscribeStageActivities(diffStage),
    api.on("activityEvent", (payload: PluginActivityEvent) => {
      // A push before any server is focused has no server to log it to (FR-11.9).
      if (payload.serverId === null) return;
      add(payload.serverId, {
        serverId: payload.serverId,
        at: Date.now(),
        kind: "pluginEvent",
        plugin: {
          pluginId: payload.pluginId,
          text: payload.text,
          actionLabel: payload.actionLabel,
          actionId: payload.actionId,
        },
      });
    }),
    api.on("connectionStateChanged", ({ serverId, state: connection }) => {
      if (connection.state !== "connected") forgetBaseline(serverId);
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

function logOf(serverId: number | null): readonly ActivityEvent[] {
  ensureBound();
  return serverId === null ? EMPTY_LOG : current(serverId);
}

/** Settings → Privacy (#594): empties the feed of every server and its saved history. */
export function clearActivityHistory(): void {
  clearSavedHistory();
  logs = new Map(Array.from(logs.keys(), (serverId) => [serverId, EMPTY_LOG]));
  notify();
}

/** The events of one server, newest first (FR-11.10); empty for `null`. */
export function useActivityLog(serverId: number | null): readonly ActivityEvent[] {
  const get = useCallback(() => logOf(serverId), [serverId]);
  return useSyncExternalStore(subscribe, get);
}

/** Test helper: forgets the binding, the events and the baselines. */
export function resetActivityLogStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  listeners.clear();
  logs = new Map();
  baselines = new Map();
}
