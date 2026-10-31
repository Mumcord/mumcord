/**
 * Saved servers and connection status for the server bar, the status bar and the server dialogs.
 * Both mounts (server bar and chat status slot) read the same state, so it lives in a small
 * module-level store that subscribes to the engine while at least one component is mounted.
 * Channel and user data is not kept here; that comes from the channels feature.
 */
import { useSyncExternalStore } from "react";
import { announce } from "../../a11y";
import {
  EngineError,
  getApi,
  type ApiError,
  type CertTrustRequired,
  type ConnectionState,
  type DeepLinkOpened,
  type Identity,
  type MumbleLink,
  type MumcordApi,
  type P2pMode,
  type SavedServer,
  type ServerInput,
  type Unsubscribe,
} from "../../api";
import { i18n } from "../../i18n";
import { getShownServerId, showServer, subscribeServerChanges } from "../channels/serverStore";
import { displayName } from "./format";

/** A dialog the server feature should show; rendered by the server bar mount. */
export type DialogRequest =
  | { kind: "add"; link: MumbleLink | null }
  | { kind: "edit"; serverId: number }
  | { kind: "remove"; serverId: number }
  | { kind: "theme"; serverId: number }
  | { kind: "identity"; serverId: number };

/** Why the last connection ended without the user asking for it. */
export interface ConnectionEnd {
  serverId: number;
  error: ApiError | null;
  /** True when the connection had been established before (lost, then given up or cancelled). */
  afterLoss: boolean;
}

export type ActionContext =
  | "reorder"
  | "remove"
  | "disconnect"
  | "alwaysConnect"
  | "notifications"
  | "savePassword"
  | "identity";

/** A server whose connect was refused for a wrong password; the retry dialog is open (FR-2.59). */
export interface PasswordPrompt {
  serverId: number;
  /** A retry with a typed password was refused again. */
  wrong: boolean;
  /** The retry is under way; the dialog waits for its outcome. */
  busy: boolean;
}

export interface ServersState {
  ready: boolean;
  servers: SavedServer[];
  /** Every identity, default first (issue #148); the server dialogs and badges read it. */
  identities: Identity[];
  /** Certificate name of the default identity, for the user panel before a connect. */
  identityName: string;
  /** A server that connected with the default because its own identity is gone (issue #148). */
  identityNotice: number | null;
  /** Connection of the shown server, or of the server that changed last while none is shown. */
  connection: ConnectionState;
  /** Every server that is not disconnected (FR-2.1). */
  connections: ReadonlyMap<number, ConnectionState>;
  transport: "udp" | "tcp" | null;
  ended: ConnectionEnd | null;
  actionError: { context: ActionContext; error: ApiError } | null;
  /** Servers waiting for a password, in the order they failed; the dialog shows the first. */
  passwordPrompts: readonly PasswordPrompt[];
  certPrompt: CertTrustRequired | null;
  dialog: DialogRequest | null;
  /** The connected-servers limit, read from the settings when the engine refused one (FR-2.1). */
  serverLimit: number | null;
}

const DISCONNECTED: ConnectionState = { state: "disconnected", error: null };

const INITIAL: ServersState = {
  ready: false,
  servers: [],
  identities: [],
  identityName: "",
  identityNotice: null,
  connection: { state: "disconnected", error: null },
  connections: new Map(),
  transport: null,
  ended: null,
  actionError: null,
  passwordPrompts: [],
  certPrompt: null,
  dialog: null,
  serverLimit: null,
};

interface Store {
  api: MumcordApi;
  state: ServersState;
  listeners: Set<() => void>;
  unsubscribers: Unsubscribe[];
  /** Set once an engine connection event arrived, so the initial query can't overwrite it. */
  connectionSeen: boolean;
  /** A link opened before the saved servers were loaded; handled once they are. */
  pendingLink: DeepLinkOpened | null;
}

let store: Store | null = null;

export function toApiError(error: unknown): ApiError {
  if (error instanceof EngineError) return { code: error.code, message: error.message };
  console.error("Unexpected server feature error", error);
  return { code: "internal", message: "" };
}

function t(key: string, options: Record<string, unknown> = {}): string {
  return i18n.t(`servers:${key}`, options);
}

function setState(patch: Partial<ServersState>): void {
  if (!store) return;
  store.state = { ...store.state, ...patch };
  for (const listener of store.listeners) listener();
}

function serverName(state: ServersState, id: number): string {
  const server = state.servers.find((s) => s.id === id);
  return server ? displayName(server) : t("unknownServer");
}

/**
 * Typed passwords to save once their connect succeeded (FR-2.59). Kept out of the state so no
 * listener or devtool sees them; dropped when the connect fails or the user cancels.
 */
const passwordsToSave = new Map<number, string>();

function withoutPrompt(prompts: readonly PasswordPrompt[], serverId: number): PasswordPrompt[] {
  return prompts.filter((p) => p.serverId !== serverId);
}

/** Opens or reopens the retry dialog; an automatic reconnect never does (FR-2.8). */
function promptPassword(state: ServersState, serverId: number): PasswordPrompt[] {
  passwordsToSave.delete(serverId);
  const index = state.passwordPrompts.findIndex((p) => p.serverId === serverId);
  const prompt = {
    serverId,
    wrong: state.passwordPrompts[index]?.busy ?? false,
    busy: false,
  };
  if (index < 0) return [...state.passwordPrompts, prompt];
  return state.passwordPrompts.map((p, i) => (i === index ? prompt : p));
}

async function savePassword(serverId: number, password: string): Promise<void> {
  const { api, state } = ensure();
  const server = state.servers.find((s) => s.id === serverId);
  if (!server) return;
  try {
    replaceServer(
      await api.serverUpdate(serverId, {
        label: server.label,
        address: server.address,
        port: server.port,
        username: server.username,
        password: { kind: "set", value: password },
      }),
    );
  } catch (error) {
    console.error("Saving the new server password failed");
    setState({ actionError: { context: "savePassword", error: toApiError(error) } });
  }
}

function onConnection(serverId: number, next: ConnectionState): void {
  if (!store) return;
  store.connectionSeen = true;
  const state = store.state;
  const prev = state.connections.get(serverId) ?? DISCONNECTED;
  const connections = new Map(state.connections);
  if (next.state === "disconnected") connections.delete(serverId);
  else connections.set(serverId, next);
  const shown = getShownServerId();
  const showsThis = shown === null || shown === serverId || !connections.has(shown);
  const patch: Partial<ServersState> = {
    connections,
    ...(showsThis ? { connection: next } : {}),
  };
  switch (next.state) {
    case "connecting":
      patch.ended = null;
      patch.transport = null;
      // The status bar (role="status") already says "Connecting to …".
      break;
    case "connected": {
      patch.ended = null;
      patch.passwordPrompts = withoutPrompt(state.passwordPrompts, serverId);
      const typed = passwordsToSave.get(serverId);
      if (typed !== undefined) {
        passwordsToSave.delete(serverId);
        void savePassword(serverId, typed);
      }
      announce(t("announce.connected", { server: serverName(state, next.serverId) }));
      break;
    }
    case "reconnecting":
      if (prev.state !== "reconnecting") {
        announce(t("announce.lost", { server: serverName(state, next.serverId) }), "assertive");
      }
      break;
    case "disconnected": {
      patch.transport = null;
      // Another server's trust question stays: with two servers connecting at once, the first
      // one failing must not leave the second waiting for an answer nobody can give.
      if (state.certPrompt?.serverId === serverId) patch.certPrompt = null;
      const prevServer = prev.state === "disconnected" ? null : prev.serverId;
      const afterLoss = prev.state === "reconnecting";
      if (
        prevServer !== null &&
        prev.state === "connecting" &&
        next.error?.code === "wrongPassword"
      ) {
        patch.passwordPrompts = promptPassword(state, prevServer);
      } else if (prevServer !== null) {
        passwordsToSave.delete(prevServer);
        patch.passwordPrompts = withoutPrompt(state.passwordPrompts, prevServer);
      }
      if (prevServer !== null && (next.error !== null || afterLoss)) {
        patch.ended = { serverId: prevServer, error: next.error, afterLoss };
        announce(t("announce.ended", { server: serverName(state, prevServer) }), "assertive");
      } else if (prevServer !== null) {
        patch.ended = null;
        announce(t("announce.disconnected", { server: serverName(state, prevServer) }));
      }
      break;
    }
  }
  setState(patch);
}

function onTransport(transport: "udp" | "tcp"): void {
  if (!store) return;
  if (transport === "tcp" && store.state.transport !== "tcp") announce(t("announce.tcp"));
  setState({ transport });
}

function start(api: MumcordApi): Store {
  const created: Store = {
    api,
    state: INITIAL,
    listeners: new Set(),
    unsubscribers: [],
    connectionSeen: false,
    pendingLink: null,
  };
  store = created;
  created.unsubscribers = [
    api.on("connectionStateChanged", ({ serverId, state }) => {
      onConnection(serverId, state);
    }),
    api.on("transportChanged", ({ transport }) => {
      onTransport(transport);
    }),
    api.on("certTrustRequired", (prompt) => {
      setState({ certPrompt: prompt });
    }),
    api.on("deepLinkOpened", (opened) => {
      openLink(opened);
    }),
    api.on("serverIdentityMissing", ({ serverId }) => {
      setState({ identityNotice: serverId });
    }),
    subscribeServerChanges((change) => {
      if (change.kind !== "shown" || store !== created) return;
      const shown = getShownServerId();
      const connection = shown === null ? undefined : created.state.connections.get(shown);
      if (connection) setState({ connection });
    }),
  ];
  // A link that launched the app arrived before these listeners existed (B25).
  void api.deepLinkTakePending().then(
    (opened) => {
      // Not tied to `created`: React StrictMode restarts the store and the link is handed out once.
      if (opened) openLink(opened);
    },
    (error: unknown) => {
      console.error("Reading the start link failed", error);
    },
  );
  void Promise.all([api.serversList(), api.connectionsList(), api.identityList()]).then(
    ([servers, connections, identities]) => {
      if (store !== created) return;
      const shown = getShownServerId();
      const map = new Map(connections.map((c) => [c.serverId, c.state]));
      const connection =
        (shown === null ? undefined : map.get(shown)) ?? connections[0]?.state ?? DISCONNECTED;
      setState({
        ready: true,
        servers,
        identities,
        identityName: identities.find((i) => i.isDefault)?.name ?? "",
        ...(created.connectionSeen ? {} : { connection, connections: map }),
      });
      takePendingLink(created);
    },
    (error: unknown) => {
      if (store !== created) return;
      console.error("Loading saved servers failed", error);
      setState({ ready: true });
      takePendingLink(created);
    },
  );
  return created;
}

function takePendingLink(current: Store): void {
  const opened = current.pendingLink;
  current.pendingLink = null;
  if (opened) openLink(opened);
}

function stop(): void {
  if (!store) return;
  for (const unsubscribe of store.unsubscribers) unsubscribe();
  store = null;
}

/** Returns the running store, (re)starting it when the engine client changed (tests). */
function ensure(): Store {
  const api = getApi();
  if (store?.api === api) return store;
  const listeners = store?.listeners ?? new Set<() => void>();
  stop();
  const started = start(api);
  started.listeners = listeners;
  return started;
}

function subscribe(listener: () => void): () => void {
  const current = ensure();
  current.listeners.add(listener);
  return () => {
    current.listeners.delete(listener);
    if (store === current && current.listeners.size === 0) stop();
  };
}

function getSnapshot(): ServersState {
  return store?.state ?? INITIAL;
}

export function useServers(): ServersState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** The user turned `@all` and `@channel` off for this server (issue #70). */
export function groupMentionsMuted(serverId: number): boolean {
  return getSnapshot().servers.find((s) => s.id === serverId)?.groupMentionsMuted ?? false;
}

/** The limit read with the last `serverLimitReached` refusal, `null` if unknown. */
export function getServerLimit(): number | null {
  return getSnapshot().serverLimit;
}

// --- actions ---

export function openDialog(dialog: DialogRequest): void {
  ensure();
  setState({ dialog });
}

export function closeDialog(): void {
  setState({ dialog: null });
}

export function dismissActionError(): void {
  setState({ actionError: null });
}

export async function addServer(input: ServerInput): Promise<SavedServer> {
  const { api } = ensure();
  const server = await api.serverAdd(input);
  setState({ servers: [...getSnapshot().servers, server] });
  return server;
}

export async function updateServer(id: number, input: ServerInput): Promise<SavedServer> {
  const { api } = ensure();
  const server = await api.serverUpdate(id, input);
  setState({ servers: getSnapshot().servers.map((s) => (s.id === id ? server : s)) });
  return server;
}

/**
 * Reads the identity list again after it changed in the settings (issue #148), together with the
 * servers: deleting an identity puts its servers back on the default, which shows in the list.
 */
export async function refreshIdentities(): Promise<void> {
  const { api } = ensure();
  try {
    const [identities, servers] = await Promise.all([api.identityList(), api.serversList()]);
    setState({
      identities,
      servers,
      identityName: identities.find((i) => i.isDefault)?.name ?? "",
    });
  } catch (error) {
    console.error("Reading the identities failed", toApiError(error).code);
  }
}

/**
 * Stores a server's identity choice (issue #148); with `reconnect` the server is disconnected
 * and connected again right away, so the change takes effect. Other servers are not touched.
 */
export async function setServerIdentity(
  serverId: number,
  identityId: number | null,
  reconnect: boolean,
): Promise<boolean> {
  const { api } = ensure();
  try {
    const server = await api.serverSetIdentity(serverId, identityId);
    setState({
      servers: getSnapshot().servers.map((s) => (s.id === serverId ? server : s)),
      actionError: null,
    });
    await refreshIdentities();
    if (reconnect) {
      if (getSnapshot().connections.has(serverId)) await api.disconnect(serverId);
      requestConnect(serverId);
    }
    return true;
  } catch (error) {
    setState({ actionError: { context: "identity", error: toApiError(error) } });
    return false;
  }
}

/** Closes the "identity missing" notice of the server bar. */
export function dismissIdentityNotice(): void {
  setState({ identityNotice: null });
}

export async function removeServer(id: number): Promise<void> {
  const { api } = ensure();
  const { connections, servers } = getSnapshot();
  try {
    if (connections.has(id)) await api.disconnect(id);
    await api.serverRemove(id);
    const ended = getSnapshot().ended;
    setState({
      servers: servers.filter((s) => s.id !== id),
      ...(ended?.serverId === id ? { ended: null } : {}),
    });
    announce(t("announce.removed", { server: serverName({ ...INITIAL, servers }, id) }));
    // The identity's "used by" list no longer counts this server.
    void refreshIdentities();
  } catch (error) {
    setState({ actionError: { context: "remove", error: toApiError(error) } });
  }
}

/** Moves a server to `toIndex`; the new order shows at once and is reverted if saving fails. */
export async function moveServer(id: number, toIndex: number): Promise<void> {
  const { api } = ensure();
  const before = getSnapshot().servers;
  const from = before.findIndex((s) => s.id === id);
  const moving = before[from];
  if (!moving || toIndex < 0 || toIndex >= before.length || from === toIndex) return;
  const after = before.filter((s) => s.id !== id);
  after.splice(toIndex, 0, moving);
  setState({ servers: after, actionError: null });
  announce(
    t("announce.moved", {
      server: moving.label || moving.address,
      position: toIndex + 1,
      count: after.length,
    }),
  );
  try {
    await api.serversReorder(after.map((s) => s.id));
  } catch (error) {
    setState({ servers: before, actionError: { context: "reorder", error: toApiError(error) } });
  }
}

/**
 * Converts an engine error; for a refusal over the connected-servers limit it first reads the
 * limit, so the message can name it (FR-2.1).
 */
async function describeError(error: unknown): Promise<ApiError> {
  return withLimit(toApiError(error));
}

async function withLimit(apiError: ApiError): Promise<ApiError> {
  if (apiError.code !== "serverLimitReached") return apiError;
  try {
    const settings = await ensure().api.settingsGet();
    setState({ serverLimit: settings.maxConnectedServers });
  } catch (settingsError) {
    console.error("Reading the connected-servers limit failed", settingsError);
    setState({ serverLimit: null });
  }
  return apiError;
}

/** Starts connecting; false when the engine refused. */
async function connectNow(serverId: number, channelPath: string[] | null): Promise<boolean> {
  const { api } = ensure();
  setState({ ended: null, actionError: null });
  try {
    await api.connect(serverId, channelPath, null);
    return true;
  } catch (error) {
    const apiError = await describeError(error);
    setState({ ended: { serverId, error: apiError, afterLoss: false } });
    return false;
  }
}

/**
 * Shows a saved server, connecting it (read-only) first if it is not connected (FR-2.6). Other
 * connected servers stay connected; the engine refuses a connect beyond the limit (FR-2.1).
 */
export function requestConnect(serverId: number, channelPath: string[] | null = null): void {
  if (ensure().state.connections.has(serverId)) {
    showServer(serverId);
    return;
  }
  void connectNow(serverId, channelPath).then((connected) => {
    if (connected) showServer(serverId);
  });
}

/**
 * Disconnects one server, by default the shown one (FR-2.9); the other servers stay connected.
 */
export async function disconnect(serverId?: number): Promise<void> {
  const { api } = ensure();
  const { connection } = getSnapshot();
  const id = serverId ?? (connection.state === "disconnected" ? null : connection.serverId);
  if (id === null) return;
  try {
    await api.disconnect(id);
  } catch (error) {
    setState({ actionError: { context: "disconnect", error: toApiError(error) } });
  }
}

function replaceServer(server: SavedServer): void {
  setState({ servers: getSnapshot().servers.map((s) => (s.id === server.id ? server : s)) });
}

/**
 * Saves the "Always connect at start" flag (FR-2.7). Throws the engine's error, e.g.
 * `serverLimitReached` when more servers would be marked than the limit allows (the limit is
 * then in `serverLimit`).
 */
export async function saveAlwaysConnect(serverId: number, enabled: boolean): Promise<void> {
  const { api } = ensure();
  try {
    replaceServer(await api.serverSetAlwaysConnect(serverId, enabled));
  } catch (error) {
    await describeError(error);
    throw error;
  }
}

/** The menu's "Always connect at start" switch; a refusal shows in the status bar. */
export async function toggleAlwaysConnect(serverId: number, enabled: boolean): Promise<void> {
  setState({ actionError: null });
  try {
    await saveAlwaysConnect(serverId, enabled);
  } catch (error) {
    setState({ actionError: { context: "alwaysConnect", error: toApiError(error) } });
  }
}

/** Sets the theme shown with the server; `null` means the app theme (FR-2.17). Throws the engine's error. */
export async function saveServerTheme(serverId: number, themeId: string | null): Promise<void> {
  const { api } = ensure();
  replaceServer(await api.serverSetTheme(serverId, themeId));
}

/** The server's own peer-to-peer mode; `null` follows the global setting (spec 04, FR-4.2). */
export async function saveP2pModeOverride(serverId: number, mode: P2pMode | null): Promise<void> {
  const { api } = ensure();
  replaceServer(await api.serverSetP2pModeOverride(serverId, mode));
}

/** The menu's "Mute notifications" switch (FR-2.10); badges keep updating. */
export async function toggleNotificationsMuted(serverId: number, muted: boolean): Promise<void> {
  const { api } = ensure();
  setState({ actionError: null });
  try {
    replaceServer(await api.serverSetNotificationsMuted(serverId, muted));
  } catch (error) {
    setState({ actionError: { context: "notifications", error: toApiError(error) } });
  }
}

/** The menu's "Mute @all and @channel" switch (issue #70, Q1.6). */
export async function toggleGroupMentionsMuted(serverId: number, muted: boolean): Promise<void> {
  const { api } = ensure();
  setState({ actionError: null });
  try {
    replaceServer(await api.serverSetGroupMentionsMuted(serverId, muted));
  } catch (error) {
    setState({ actionError: { context: "notifications", error: toApiError(error) } });
  }
}

export async function cancelReconnect(): Promise<void> {
  const { api } = ensure();
  const { connection } = getSnapshot();
  if (connection.state === "disconnected") return;
  try {
    await api.reconnectCancel(connection.serverId);
  } catch (error) {
    setState({ actionError: { context: "disconnect", error: toApiError(error) } });
  }
}

/**
 * Retries a refused connect with the typed password for this connect only (FR-2.59). With
 * `save` the saved server gets the password once the connect succeeded.
 */
export async function retryWithPassword(
  serverId: number,
  password: string,
  save: boolean,
): Promise<void> {
  const { api, state } = ensure();
  if (save) passwordsToSave.set(serverId, password);
  else passwordsToSave.delete(serverId);
  setState({
    ended: null,
    passwordPrompts: state.passwordPrompts.map((p) =>
      p.serverId === serverId ? { ...p, busy: true } : p,
    ),
  });
  try {
    await api.connect(serverId, null, password);
  } catch (error) {
    passwordsToSave.delete(serverId);
    const apiError = await describeError(error);
    setState({
      ended: { serverId, error: apiError, afterLoss: false },
      passwordPrompts: withoutPrompt(ensure().state.passwordPrompts, serverId),
    });
  }
}

/** Closes the retry dialog; the server stays disconnected with its normal error. */
export function cancelPasswordPrompt(serverId: number): void {
  passwordsToSave.delete(serverId);
  setState({ passwordPrompts: withoutPrompt(ensure().state.passwordPrompts, serverId) });
}

/** Answers a certificate prompt; trusting is remembered by the engine (FR-1.10). */
export async function decideTrust(trust: boolean): Promise<void> {
  const { api, state } = ensure();
  const prompt = state.certPrompt;
  if (!prompt) return;
  setState({ certPrompt: null });
  try {
    await api.certTrustDecide(prompt.serverId, prompt.fingerprint, trust);
  } catch (error) {
    setState({ ended: { serverId: prompt.serverId, error: toApiError(error), afterLoss: false } });
  }
}

/**
 * Shows the result of a `mumble://` link the engine already handled (FR-1.11, FR-2.13): it
 * connected or joined a matching server, which is now shown; an unknown server is offered for
 * saving; a refusal is explained on the server's status.
 */
export function openLink({ link, outcome }: DeepLinkOpened): void {
  const current = ensure();
  if (!current.state.ready) {
    current.pendingLink = { link, outcome };
    return;
  }
  switch (outcome.kind) {
    case "shown":
    case "connecting":
      showServer(outcome.serverId);
      return;
    case "unknownServer":
      setState({ dialog: { kind: "add", link } });
      return;
    case "limitReached":
      showServer(outcome.serverId);
      void withLimit({ code: "serverLimitReached", message: "" }).then((error) => {
        setState({ ended: { serverId: outcome.serverId, error, afterLoss: false } });
      });
      return;
    case "failed":
      showServer(outcome.serverId);
      setState({ ended: { serverId: outcome.serverId, error: outcome.error, afterLoss: false } });
      return;
  }
}
