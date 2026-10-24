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
  type MumbleLink,
  type MumcordApi,
  type SavedServer,
  type ServerInput,
  type Unsubscribe,
} from "../../api";
import { i18n } from "../../i18n";

/** A dialog the server feature should show; rendered by the server bar mount. */
export type DialogRequest =
  | { kind: "add"; link: MumbleLink | null }
  | { kind: "edit"; serverId: number }
  | { kind: "remove"; serverId: number }
  | { kind: "switch"; serverId: number; channelPath: string[] | null };

/** Why the last connection ended without the user asking for it. */
export interface ConnectionEnd {
  serverId: number;
  error: ApiError | null;
  /** True when the connection had been established before (lost, then given up or cancelled). */
  afterLoss: boolean;
}

export type ActionContext = "reorder" | "remove" | "disconnect";

export interface ServersState {
  ready: boolean;
  servers: SavedServer[];
  identityName: string;
  connection: ConnectionState;
  transport: "udp" | "tcp" | null;
  ended: ConnectionEnd | null;
  actionError: { context: ActionContext; error: ApiError } | null;
  certPrompt: CertTrustRequired | null;
  dialog: DialogRequest | null;
}

const INITIAL: ServersState = {
  ready: false,
  servers: [],
  identityName: "",
  connection: { state: "disconnected", error: null },
  transport: null,
  ended: null,
  actionError: null,
  certPrompt: null,
  dialog: null,
};

interface Store {
  api: MumcordApi;
  state: ServersState;
  listeners: Set<() => void>;
  unsubscribers: Unsubscribe[];
  /** Set once an engine connection event arrived, so the initial query can't overwrite it. */
  connectionSeen: boolean;
  /** A link opened before the saved servers were loaded; handled once they are. */
  pendingLink: MumbleLink | null;
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
  return server ? server.label || server.address : t("unknownServer");
}

function onConnection(next: ConnectionState): void {
  if (!store) return;
  store.connectionSeen = true;
  const state = store.state;
  const prev = state.connection;
  const patch: Partial<ServersState> = { connection: next };
  switch (next.state) {
    case "connecting":
      patch.ended = null;
      patch.transport = null;
      // The status bar (role="status") already says "Connecting to …".
      break;
    case "connected":
      patch.ended = null;
      announce(t("announce.connected", { server: serverName(state, next.serverId) }));
      break;
    case "reconnecting":
      if (prev.state !== "reconnecting") {
        announce(t("announce.lost", { server: serverName(state, next.serverId) }), "assertive");
      }
      break;
    case "disconnected": {
      patch.transport = null;
      patch.certPrompt = null;
      const prevServer = prev.state === "disconnected" ? null : prev.serverId;
      const afterLoss = prev.state === "reconnecting";
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
    api.on("connectionStateChanged", ({ state }) => {
      onConnection(state);
    }),
    api.on("transportChanged", ({ transport }) => {
      onTransport(transport);
    }),
    api.on("certTrustRequired", (prompt) => {
      setState({ certPrompt: prompt });
    }),
    api.on("deepLinkOpened", ({ link }) => {
      openLink(link);
    }),
  ];
  // A link that launched the app arrived before these listeners existed (B25).
  void api.deepLinkTakePending().then(
    (link) => {
      // Not tied to `created`: React StrictMode restarts the store and the link is handed out once.
      if (link) openLink(link);
    },
    (error: unknown) => {
      console.error("Reading the start link failed", error);
    },
  );
  void Promise.all([api.serversList(), api.connectionState(), api.identityGet()]).then(
    ([servers, connection, identity]) => {
      if (store !== created) return;
      setState({
        ready: true,
        servers,
        identityName: identity?.name ?? "",
        ...(created.connectionSeen ? {} : { connection }),
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
  const link = current.pendingLink;
  current.pendingLink = null;
  if (link) openLink(link);
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

export async function removeServer(id: number): Promise<void> {
  const { api } = ensure();
  const { connection, servers } = getSnapshot();
  try {
    if (connection.state !== "disconnected" && connection.serverId === id) await api.disconnect();
    await api.serverRemove(id);
    const ended = getSnapshot().ended;
    setState({
      servers: servers.filter((s) => s.id !== id),
      ...(ended?.serverId === id ? { ended: null } : {}),
    });
    announce(t("announce.removed", { server: serverName({ ...INITIAL, servers }, id) }));
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

async function connectNow(serverId: number, channelPath: string[] | null): Promise<void> {
  const { api } = ensure();
  setState({ ended: null, actionError: null });
  try {
    await api.connect(serverId, channelPath);
  } catch (error) {
    setState({ ended: { serverId, error: toApiError(error), afterLoss: false } });
  }
}

/**
 * Connects to a saved server. Only one server can be connected (FR-1.9): while another one is
 * active, the user is asked to switch first.
 */
export function requestConnect(serverId: number, channelPath: string[] | null = null): void {
  const { connection } = ensure().state;
  if (connection.state !== "disconnected") {
    if (connection.serverId === serverId) return;
    setState({ dialog: { kind: "switch", serverId, channelPath } });
    return;
  }
  void connectNow(serverId, channelPath);
}

/** Confirmed switch: the engine disconnects the current server before connecting. */
export function confirmSwitch(serverId: number, channelPath: string[] | null): void {
  setState({ dialog: null });
  void connectNow(serverId, channelPath);
}

export async function disconnect(): Promise<void> {
  const { api } = ensure();
  try {
    await api.disconnect();
  } catch (error) {
    setState({ actionError: { context: "disconnect", error: toApiError(error) } });
  }
}

export async function cancelReconnect(): Promise<void> {
  const { api } = ensure();
  try {
    await api.reconnectCancel();
  } catch (error) {
    setState({ actionError: { context: "disconnect", error: toApiError(error) } });
  }
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

function sameHost(a: string, b: string): boolean {
  return a.trim().toLowerCase() === b.trim().toLowerCase();
}

/**
 * Handles a `mumble://` link (FR-1.11): connects to a matching saved server, otherwise offers to
 * save it first.
 */
export function openLink(link: MumbleLink): void {
  const current = ensure();
  if (!current.state.ready) {
    current.pendingLink = link;
    return;
  }
  const { servers } = current.state;
  const match = servers.find(
    (s) =>
      sameHost(s.address, link.address) &&
      s.port === link.port &&
      (link.username === null || s.username === link.username),
  );
  if (match) requestConnect(match.id, link.channelPath.length > 0 ? link.channelPath : null);
  else setState({ dialog: { kind: "add", link } });
}
