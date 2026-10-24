/**
 * In-memory engine for building and testing the UI before the real engine exists.
 * It follows the contract in docs/engine-api.md closely enough for UI flows, not the
 * Mumble protocol: no network, no audio.
 */
import type {
  ApiError,
  AudioDevices,
  ChannelInfo,
  ChatMessage,
  ConnectionState,
  ConnectionStats,
  Conversation,
  ErrorCode,
  HistoryScope,
  Identity,
  MumbleLink,
  PttAvailability,
  PttMouseStatus,
  SavedServer,
  ServerInfo,
  ServerInput,
  ServerSnapshot,
  Settings,
  User,
} from "../bindings/commands";
import {
  EngineError,
  type CommandName,
  type EngineCommands,
  type EventName,
  type EventPayloads,
  type MumcordApi,
  type Unsubscribe,
} from "./client";

export interface MockOptions {
  /** "fresh" simulates the first start (no identity, no servers, wizard pending). */
  seed?: "fresh" | "ready";
  /** Simulated server: protocol version and listener limit. */
  serverVersion?: string;
  maxListeners?: number;
  /** Delay before connection events fire; 0 emits them synchronously (handy in tests). */
  connectDelayMs?: number;
}

/** Test and dev controls on top of the normal API. */
export interface MockControls {
  emit<K extends EventName>(event: K, payload: EventPayloads[K]): void;
  /** Makes the next call of `command` fail with the given error. */
  failNext(command: CommandName, code: ErrorCode, message?: string): void;
  /** The next connect to `serverId` asks for certificate trust first. */
  requireCertTrust(serverId: number, previousFingerprint?: string): void;
  /** Simulates an incoming message from another person. */
  receiveMessage(conversation: Conversation, body: string, senderSession: number): ChatMessage;
  /** Simulates a dropped connection; the mock enters `reconnecting`. */
  loseConnection(): void;
  /** Ends a reconnect attempt successfully. */
  completeReconnect(): void;
  setTalking(session: number, talking: boolean): void;
  /** Changes what `connectionStats` reports while connected (e.g. a lower bitrate). */
  setConnectionStats(stats: Partial<ConnectionStats>): void;
  /**
   * Simulates that the app last quit while joined to `channelPath` on `serverId`: the next fresh
   * connect to that server offers to rejoin it (QA.9).
   */
  offerRejoin(serverId: number, channelPath: string[]): void;
}

export type MockApi = MumcordApi & { readonly mock: MockControls };

const SELF_SESSION = 1;
const MOCK_FINGERPRINT = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99";

const DEFAULT_SETTINGS: Settings = {
  wizardCompleted: false,
  theme: "system",
  language: null,
  talkMode: "voiceActivation",
  pttKey: null,
  allowMouseButtons: false,
  pttCompatibilityMode: false,
  inputDevice: null,
  outputDevice: null,
  noiseSuppression: true,
  echoCancellation: true,
  autoGain: true,
  sounds: { joinLeave: true, selfMute: true, selfDeaf: true, messages: true },
  closeBehaviour: "ask",
  logLevel: "info",
  sendDefaultAvatar: true,
};

/** 1×1 PNG standing in for any image chosen as the default avatar. */
const MOCK_DEFAULT_AVATAR =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

const DEVICES: AudioDevices = {
  inputs: [
    { id: "mic-default", name: "Built-in Microphone", isDefault: true },
    { id: "mic-usb", name: "USB Headset", isDefault: false },
  ],
  outputs: [
    { id: "out-default", name: "Built-in Speakers", isDefault: true },
    { id: "out-usb", name: "USB Headset", isDefault: false },
  ],
};

const DEFAULT_CONNECTION_STATS: ConnectionStats = {
  transport: "udp",
  rttMs: 24.5,
  lossPercent: 0,
  sendBitrateKbps: 72,
  serverMaxBandwidthKbps: 558,
  packetsSent: 0,
  packetsReceived: 0,
};

const allow = { enter: true, textMessage: true, listen: true, registerSelf: false };

function seedChannels(): ChannelInfo[] {
  return [
    {
      id: 0,
      parentId: null,
      name: "Root",
      position: 0,
      permissions: { ...allow, registerSelf: true },
    },
    { id: 1, parentId: 0, name: "Lobby", position: 0, permissions: { ...allow } },
    { id: 2, parentId: 0, name: "Raid", position: 1, permissions: { ...allow } },
    { id: 3, parentId: 0, name: "AFK", position: 2, permissions: { ...allow, textMessage: false } },
  ];
}

function person(session: number, name: string, channelId: number, certHash: string | null): User {
  return {
    session,
    channelId,
    name,
    certHash,
    registered: certHash !== null,
    selfMute: false,
    selfDeaf: false,
    serverMute: false,
    serverDeaf: false,
    listeningChannelIds: [],
    hasAvatar: false,
    avatarHash: null,
    hasComment: false,
    volume: 100,
    localMute: false,
  };
}

function fail(code: ErrorCode, message: string): never {
  throw new EngineError(code, message);
}

function conversationKey(c: Conversation): string {
  switch (c.kind) {
    case "channel":
      return `channel:${String(c.channelId)}`;
    case "private":
      return `private:${c.certHash}`;
    case "privateSession":
      return `session:${String(c.session)}`;
  }
}

function parseLink(url: string): MumbleLink {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return fail("invalidInput", "not a valid link");
  }
  if (parsed.protocol !== "mumble:" || parsed.hostname === "") {
    return fail("invalidInput", "not a mumble:// link");
  }
  return {
    address: parsed.hostname,
    port: parsed.port === "" ? 64738 : Number(parsed.port),
    username: parsed.username === "" ? null : decodeURIComponent(parsed.username),
    password: parsed.password === "" ? null : decodeURIComponent(parsed.password),
    channelPath: parsed.pathname
      .split("/")
      .filter((part) => part !== "")
      .map(decodeURIComponent),
  };
}

/** Length of each mic-test phase (recording, playback) in the mock. */
const MIC_TEST_PHASE_MS = 5000;

export function createMockApi(options: MockOptions = {}): MockApi {
  const fresh = options.seed === "fresh";
  const serverVersion = options.serverVersion ?? "1.5.735";
  const maxListeners = options.maxListeners ?? 2;
  const connectDelayMs = options.connectDelayMs ?? 0;

  const listeners = new Map<EventName, Set<(payload: never) => void>>();
  const failures = new Map<CommandName, ApiError>();
  const certPrompts = new Map<number, string | null>();

  let identity: Identity | null = fresh ? null : { name: "Zoe", certHash: "selfhash" };
  let settings: Settings = { ...DEFAULT_SETTINGS, wizardCompleted: !fresh };
  let defaultAvatar: string | null = null;
  let servers: SavedServer[] = fresh
    ? []
    : [
        {
          id: 1,
          label: "Local test server",
          address: "localhost",
          port: 64738,
          username: "Zoe",
          hasPassword: false,
          saveHistory: true,
        },
      ];
  let nextServerId = 2;
  let nextMessageId = 1;
  let linkHandler = false;
  let state: ConnectionState = { state: "disconnected", error: null };
  let snapshot: ServerSnapshot | null = null;
  let connectionStats: ConnectionStats = { ...DEFAULT_CONNECTION_STATS };
  let micTimer: ReturnType<typeof setInterval> | undefined;
  let micPhaseTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingConnect: { serverId: number; channelPath: string[] | null } | null = null;
  let muteBeforeDeaf = false;
  // Like the engine (QA.7): the forced read-only state of a fresh connect, and the user's last
  // own mute/deafen choice while joined for voice, which a join restores.
  let readOnly = false;
  let ownVoice = { mute: false, deaf: false };
  const rejoinOffers = new Map<number, string[]>();
  // History per server id, keyed by conversation.
  const history = new Map<number, Map<string, ChatMessage[]>>();

  function emit<K extends EventName>(event: K, payload: EventPayloads[K]): void {
    const set = listeners.get(event);
    if (!set) return;
    for (const handler of set) (handler as (p: EventPayloads[K]) => void)(payload);
  }

  function on<K extends EventName>(
    event: K,
    handler: (payload: EventPayloads[K]) => void,
  ): Unsubscribe {
    let set = listeners.get(event);
    if (!set) {
      set = new Set();
      listeners.set(event, set);
    }
    const stored = handler as (payload: never) => void;
    set.add(stored);
    return () => {
      set.delete(stored);
    };
  }

  function setState(next: ConnectionState): void {
    state = next;
    emit("connectionStateChanged", { state: next });
  }

  function requireServer(id: number): SavedServer {
    return servers.find((s) => s.id === id) ?? fail("notFound", `no saved server ${String(id)}`);
  }

  function requireSnapshot(): ServerSnapshot {
    return snapshot ?? fail("notConnected", "not connected");
  }

  function requireIdentity(): Identity {
    return identity ?? fail("notFound", "no identity yet");
  }

  function requireChannel(id: number): ChannelInfo {
    return (
      requireSnapshot().channels.find((c) => c.id === id) ??
      fail("notFound", `no channel ${String(id)}`)
    );
  }

  function requireUser(session: number): User {
    return (
      requireSnapshot().users.find((u) => u.session === session) ??
      fail("notFound", `no person ${String(session)}`)
    );
  }

  function updateUser(session: number, patch: Partial<User>): User {
    const snap = requireSnapshot();
    const updated = { ...requireUser(session), ...patch };
    snap.users = snap.users.map((u) => (u.session === session ? updated : u));
    emit("userUpserted", { user: updated });
    return updated;
  }

  function selfUser(): User {
    return requireUser(SELF_SESSION);
  }

  function serverHistory(serverId: number): Map<string, ChatMessage[]> {
    let map = history.get(serverId);
    if (!map) {
      map = new Map();
      history.set(serverId, map);
    }
    return map;
  }

  function store(message: ChatMessage): void {
    const snap = requireSnapshot();
    if (!requireServer(snap.serverId).saveHistory) return;
    const map = serverHistory(snap.serverId);
    const key = conversationKey(message.conversation);
    map.set(key, [...(map.get(key) ?? []), message]);
  }

  function checkSize(html: string): void {
    const info = requireSnapshot().info;
    const limit = html.includes("<img") ? info.maxImageMessageLength : info.maxMessageLength;
    if (limit > 0 && html.length > limit) {
      fail("messageTooLarge", `message exceeds the server limit of ${String(limit)} characters`);
    }
  }

  function findChannelByPath(channels: ChannelInfo[], path: string[]): ChannelInfo | undefined {
    let current = channels.find((c) => c.parentId === null);
    for (const name of path) {
      const parentId = current?.id;
      current = channels.find((c) => c.parentId === parentId && c.name === name);
      if (!current) return undefined;
    }
    return current;
  }

  /** Voice state after a join: the remembered own choice. */
  function joinedVoice(): { selfMute: boolean; selfDeaf: boolean } {
    muteBeforeDeaf = ownVoice.deaf && ownVoice.mute;
    return { selfMute: ownVoice.mute || ownVoice.deaf, selfDeaf: ownVoice.deaf };
  }

  /** Applies an own mute/deafen toggle; only toggles while joined for voice are remembered. */
  function changeSelfVoice(change: { selfMute: boolean; selfDeaf?: boolean }): void {
    const remember = !readOnly;
    updateUser(SELF_SESSION, change);
    const self = selfUser();
    if (!self.selfDeaf) readOnly = false;
    if (remember) {
      ownVoice = {
        mute: self.selfDeaf ? muteBeforeDeaf : self.selfMute,
        deaf: self.selfDeaf,
      };
    }
  }

  function buildSnapshot(serverId: number, channelPath: string[] | null): ServerSnapshot {
    const channels = seedChannels();
    const target = (channelPath && findChannelByPath(channels, channelPath)) ?? channels[1];
    // Like the engine: a fresh connect lands read-only (muted + deafened), a link path joins.
    readOnly = channelPath === null;
    // Like the engine: offered once, on the first fresh connect, if the channel still exists.
    const offerPath = rejoinOffers.get(serverId);
    rejoinOffers.delete(serverId);
    const offer = readOnly && offerPath ? findChannelByPath(channels, offerPath) : undefined;
    const self = {
      ...person(SELF_SESSION, requireServer(serverId).username, target?.id ?? 0, "selfhash"),
      ...(readOnly ? { selfMute: true, selfDeaf: true } : joinedVoice()),
    };
    if (readOnly) muteBeforeDeaf = false;
    const anna = {
      ...person(2, "Anna", 1, "annahash"),
      hasComment: true,
      hasAvatar: true,
      avatarHash: "a1",
    };
    const carla = { ...person(4, "Carla", 2, "carlahash"), listeningChannelIds: [1] };
    const info: ServerInfo = {
      version: serverVersion,
      followSupported: !serverVersion.startsWith("1.3") && !serverVersion.startsWith("1.2"),
      welcomeText: "Welcome to the <b>mock</b> server.",
      maxMessageLength: 5000,
      maxImageMessageLength: 131072,
      allowHtml: true,
    };
    return {
      serverId,
      selfSession: SELF_SESSION,
      info,
      channels,
      users: [self, anna, person(3, "Ben", 1, null), carla],
      followedChannelIds: [],
      unread: [],
      readOnly,
      rejoinChannelId: offer?.id ?? null,
    };
  }

  function finishConnect(serverId: number, channelPath: string[] | null): void {
    snapshot = buildSnapshot(serverId, channelPath);
    setState({ state: "connected", serverId });
    emit("transportChanged", { transport: "udp" });
    emit("serverSynced", { snapshot });
  }

  function proceedConnect(serverId: number, channelPath: string[] | null): void {
    if (certPrompts.has(serverId)) {
      pendingConnect = { serverId, channelPath };
      emit("certTrustRequired", {
        serverId,
        fingerprint: MOCK_FINGERPRINT,
        previousFingerprint: certPrompts.get(serverId) ?? null,
      });
      return;
    }
    finishConnect(serverId, channelPath);
  }

  function later(fn: () => void): void {
    if (connectDelayMs === 0) fn();
    else setTimeout(fn, connectDelayMs);
  }

  function serverFields(
    input: ServerInput,
  ): Omit<SavedServer, "id" | "hasPassword" | "saveHistory"> {
    if (input.address.trim() === "") fail("invalidInput", "address is required");
    if (input.username.trim() === "") fail("invalidInput", "username is required");
    if (input.port === 0) fail("invalidInput", "port must be between 1 and 65535");
    return {
      label: input.label.trim() === "" ? input.address : input.label,
      address: input.address,
      port: input.port,
      username: input.username,
    };
  }

  function setUnread(conversation: Conversation, count: number): void {
    const snap = requireSnapshot();
    const key = conversationKey(conversation);
    snap.unread = [
      ...snap.unread.filter((u) => conversationKey(u.conversation) !== key),
      ...(count > 0 ? [{ conversation, count }] : []),
    ];
    emit("unreadChanged", { conversation, count });
  }

  function sendMessage(conversation: Conversation, html: string): ChatMessage {
    checkSize(html);
    const message: ChatMessage = {
      id: String(nextMessageId++),
      conversation,
      senderName: selfUser().name,
      senderCertHash: requireIdentity().certHash,
      senderSession: SELF_SESSION,
      own: true,
      sentAt: new Date().toISOString(),
      body: html,
    };
    store(message);
    return message;
  }

  const commands: EngineCommands = {
    // identity
    identityGet: () => Promise.resolve(identity),
    identityCreate: (name) => {
      if (name.trim() === "") fail("invalidInput", "name is required");
      identity = { name: name.trim(), certHash: "selfhash" };
      return Promise.resolve(identity);
    },
    identityImport: (path) => {
      const file = path.split(/[\\/]/).pop() ?? "Imported";
      identity = { name: file.replace(/\.p12$|\.pfx$/i, ""), certHash: "importedhash" };
      return Promise.resolve(identity);
    },
    identityExport: (_path, password) => {
      requireIdentity();
      if (password === "") fail("invalidInput", "a password is required");
      return Promise.resolve(null);
    },

    // servers
    serversList: () => Promise.resolve(servers.map((s) => ({ ...s }))),
    serverAdd: (input) => {
      const server: SavedServer = {
        id: nextServerId++,
        ...serverFields(input),
        hasPassword: input.password.kind === "set",
        saveHistory: true,
      };
      servers = [...servers, server];
      return Promise.resolve({ ...server });
    },
    serverUpdate: (id, input) => {
      const existing = requireServer(id);
      const hasPassword =
        input.password.kind === "keep" ? existing.hasPassword : input.password.kind === "set";
      const updated: SavedServer = { ...existing, ...serverFields(input), hasPassword };
      servers = servers.map((s) => (s.id === id ? updated : s));
      return Promise.resolve({ ...updated });
    },
    serverRemove: (id) => {
      requireServer(id);
      servers = servers.filter((s) => s.id !== id);
      history.delete(id);
      return Promise.resolve(null);
    },
    serversReorder: (ids) => {
      const same =
        ids.length === servers.length &&
        new Set(ids).size === ids.length &&
        ids.every((id) => servers.some((s) => s.id === id));
      if (!same) fail("invalidInput", "ids must list every saved server exactly once");
      servers = ids.map((id) => requireServer(id));
      return Promise.resolve(null);
    },
    parseMumbleLink: (url) => Promise.resolve(parseLink(url)),

    // session
    connect: (serverId, channelPath) => {
      requireServer(serverId);
      requireIdentity();
      snapshot = null;
      setState({ state: "connecting", serverId });
      later(() => {
        proceedConnect(serverId, channelPath);
      });
      return Promise.resolve(null);
    },
    disconnect: () => {
      snapshot = null;
      pendingConnect = null;
      setState({ state: "disconnected", error: null });
      return Promise.resolve(null);
    },
    reconnectCancel: () => {
      if (state.state === "reconnecting") {
        snapshot = null;
        setState({ state: "disconnected", error: null });
      }
      return Promise.resolve(null);
    },
    connectionState: () => Promise.resolve(state),
    sessionSnapshot: () => Promise.resolve(snapshot),
    connectionStats: () =>
      Promise.resolve(state.state === "connected" ? { ...connectionStats } : null),
    certTrustDecide: (serverId, _fingerprint, trust) => {
      const pending = pendingConnect;
      if (pending?.serverId !== serverId) fail("invalidInput", "no trust decision pending");
      pendingConnect = null;
      if (trust) {
        certPrompts.delete(serverId);
        finishConnect(serverId, pending.channelPath);
      } else {
        setState({
          state: "disconnected",
          error: { code: "certificateRejected", message: "certificate not trusted" },
        });
      }
      return Promise.resolve(null);
    },
    setSelfMute: (muted) => {
      const self = selfUser();
      changeSelfVoice(
        muted || !self.selfDeaf ? { selfMute: muted } : { selfMute: false, selfDeaf: false },
      );
      return Promise.resolve(null);
    },
    setSelfDeaf: (deafened) => {
      const self = selfUser();
      if (deafened) {
        if (!self.selfDeaf) muteBeforeDeaf = self.selfMute;
        changeSelfVoice({ selfDeaf: true, selfMute: true });
      } else {
        changeSelfVoice({ selfDeaf: false, selfMute: muteBeforeDeaf });
      }
      return Promise.resolve(null);
    },

    // channels
    channelJoin: (channelId) => {
      if (!requireChannel(channelId).permissions.enter) {
        fail("permissionDenied", "You may not enter this channel.");
      }
      // An explicit join turns voice on with the last own mute/deafen choice.
      readOnly = false;
      if (snapshot) snapshot = { ...snapshot, readOnly: false, rejoinChannelId: null };
      updateUser(SELF_SESSION, { channelId, ...joinedVoice() });
      return Promise.resolve(null);
    },

    // people
    personSetVolume: (session, volume) => {
      if (!Number.isInteger(volume) || volume < 0 || volume > 200) {
        fail("invalidInput", "volume must be between 0 and 200");
      }
      updateUser(session, { volume });
      return Promise.resolve(null);
    },
    personSetLocalMute: (session, muted) => {
      updateUser(session, { localMute: muted });
      return Promise.resolve(null);
    },
    personProfile: (session) => {
      const user = requireUser(session);
      return Promise.resolve({
        comment: user.hasComment ? `<p>Hi, I am ${user.name}.</p>` : null,
        avatar: user.hasAvatar
          ? "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAd0lEQVR42u3UwQ2AMAwDwE7HGt2DCRmDTdw/KIlFHIRQrHzrE5Qwzn2WzmiggQZoALcoARjRAHCTBUDkOQA6H35FAuCYmz8+EB7/DYCCXJ+gov1dQGiYl1wOSIz4MxW2m3ugavcWTdIeb3Kmmv1VOAxzlgIyUw4sffQx8QiPKKAAAAAASUVORK5CYII="
          : null,
      });
    },

    // chat
    sendChannelMessage: (channelId, html) => {
      if (!requireChannel(channelId).permissions.textMessage) {
        fail("permissionDenied", "You may not write in this channel.");
      }
      return Promise.resolve(sendMessage({ kind: "channel", channelId }, html));
    },
    sendPrivateMessage: (session, html) => {
      const user = requireUser(session);
      const conversation: Conversation =
        user.certHash === null
          ? { kind: "privateSession", session }
          : { kind: "private", certHash: user.certHash };
      return Promise.resolve(sendMessage(conversation, html));
    },

    // follow
    channelFollow: (channelId) => {
      const snap = requireSnapshot();
      if (!snap.info.followSupported) fail("serverTooOld", "following needs server 1.4 or newer");
      if (!requireChannel(channelId).permissions.listen) {
        fail("permissionDenied", "You may not listen to this channel.");
      }
      if (snap.followedChannelIds.includes(channelId)) return Promise.resolve(null);
      if (snap.followedChannelIds.length >= maxListeners) {
        fail("listenerLimitReached", "the server's listener limit is reached");
      }
      snap.followedChannelIds = [...snap.followedChannelIds, channelId];
      updateUser(SELF_SESSION, { listeningChannelIds: snap.followedChannelIds });
      emit("followedChannelsChanged", { channelIds: snap.followedChannelIds });
      return Promise.resolve(null);
    },
    channelUnfollow: (channelId) => {
      const snap = requireSnapshot();
      snap.followedChannelIds = snap.followedChannelIds.filter((id) => id !== channelId);
      updateUser(SELF_SESSION, { listeningChannelIds: snap.followedChannelIds });
      emit("followedChannelsChanged", { channelIds: snap.followedChannelIds });
      return Promise.resolve(null);
    },
    conversationMarkRead: (conversation) => {
      setUnread(conversation, 0);
      return Promise.resolve(null);
    },

    // history
    historyLoad: (conversation, before, limit) => {
      const all =
        serverHistory(requireSnapshot().serverId).get(conversationKey(conversation)) ?? [];
      const end = before === null ? all.length : all.findIndex((m) => m.id === before);
      const stop = end === -1 ? all.length : end;
      return Promise.resolve(all.slice(Math.max(0, stop - limit), stop));
    },
    historyClear: (scope: HistoryScope) => {
      if (scope.kind === "all") history.clear();
      else if (scope.kind === "server") history.delete(scope.serverId);
      else serverHistory(scope.serverId).delete(conversationKey(scope.conversation));
      return Promise.resolve(null);
    },
    historySetSaving: (serverId, enabled) => {
      requireServer(serverId);
      servers = servers.map((s) => (s.id === serverId ? { ...s, saveHistory: enabled } : s));
      return Promise.resolve(null);
    },

    // profile
    setOwnAvatar: (path) => {
      updateUser(SELF_SESSION, {
        hasAvatar: path !== null,
        avatarHash: path === null ? null : `own-${path}`,
      });
      return Promise.resolve(null);
    },
    setOwnComment: (html) => {
      checkSize(html);
      updateUser(SELF_SESSION, { hasComment: html !== "" });
      return Promise.resolve(null);
    },
    defaultAvatarGet: () => Promise.resolve(defaultAvatar),
    defaultAvatarSet: (path) => {
      defaultAvatar = path === null ? null : MOCK_DEFAULT_AVATAR;
      return Promise.resolve(defaultAvatar);
    },
    registerSelf: () => {
      const root = requireSnapshot().channels.find((c) => c.parentId === null);
      if (!root?.permissions.registerSelf) fail("permissionDenied", "Registration is not allowed.");
      updateUser(SELF_SESSION, { registered: true });
      return Promise.resolve(null);
    },

    // settings
    settingsGet: () => Promise.resolve({ ...settings }),
    settingsUpdate: (next) => {
      settings = { ...next };
      return Promise.resolve({ ...settings });
    },

    // audio
    audioDevices: () => Promise.resolve(DEVICES),
    micTestStart: () => {
      clearInterval(micTimer);
      clearTimeout(micPhaseTimer);
      micTimer = setInterval(() => {
        emit("micLevel", { level: Math.random() * 0.6 });
      }, 50);
      emit("micTestPhaseChanged", { phase: "recording" });
      micPhaseTimer = setTimeout(() => {
        emit("micTestPhaseChanged", { phase: "playing" });
        micPhaseTimer = setTimeout(() => {
          clearInterval(micTimer);
          micTimer = undefined;
          emit("micTestPhaseChanged", { phase: "done" });
        }, MIC_TEST_PHASE_MS);
      }, MIC_TEST_PHASE_MS);
      return Promise.resolve(null);
    },
    micTestStop: () => {
      clearInterval(micTimer);
      clearTimeout(micPhaseTimer);
      micTimer = undefined;
      return Promise.resolve(null);
    },

    // push-to-talk
    pttAvailability: () => Promise.resolve<PttAvailability>({ status: "available" }),
    // Mouse buttons come back unchanged (no desktop dialog), keyboard keys as "F13".
    pttCaptureKey: (key) => Promise.resolve(/^Mouse(Middle|4|5)$/.test(key) ? key : "F13"),
    pttMouseStatus: () => Promise.resolve<PttMouseStatus>({ support: "native", state: "ready" }),
    pttCaptureCancel: () => Promise.resolve(null),
    pttSession: () => Promise.resolve({ waylandDesktop: false, compatibilityActive: false }),

    // app
    appRestart: () => Promise.resolve(null),
    closeChoice: (choice, remember) => {
      if (remember) settings = { ...settings, closeBehaviour: choice };
      return Promise.resolve(null);
    },
    linkHandlerStatus: () => Promise.resolve(linkHandler),
    linkHandlerRegister: () => {
      linkHandler = true;
      return Promise.resolve(null);
    },
    // Mock links arrive only via `api.mock.emit("deepLinkOpened", …)`.
    deepLinkTakePending: () => Promise.resolve(null),

    // diagnostics
    diagnosticsSave: () => Promise.resolve(null),
    appInfo: () =>
      Promise.resolve({
        version: "0.0.0-mock",
        os: "mock",
        thirdPartyNotices:
          "Mumble\nCopyright (c) The Mumble Developers\nBSD-3-Clause\n\nSome library - Apache-2.0",
      }),
  };

  // Turns synchronous throws into rejections and applies injected failures.
  const guarded = Object.fromEntries(
    (Object.keys(commands) as CommandName[]).map((name) => {
      const impl = commands[name] as (...args: unknown[]) => Promise<unknown>;
      const run = async (...args: unknown[]): Promise<unknown> => {
        const injected = failures.get(name);
        if (injected) {
          failures.delete(name);
          throw new EngineError(injected.code, injected.message);
        }
        return impl(...args);
      };
      return [name, run];
    }),
  ) as EngineCommands;

  const mock: MockControls = {
    emit,
    failNext: (command, code, message = "injected failure") => {
      failures.set(command, { code, message });
    },
    requireCertTrust: (serverId, previousFingerprint) => {
      certPrompts.set(serverId, previousFingerprint ?? null);
    },
    receiveMessage: (conversation, body, senderSession) => {
      const sender = requireUser(senderSession);
      const message: ChatMessage = {
        id: String(nextMessageId++),
        conversation,
        senderName: sender.name,
        senderCertHash: sender.certHash,
        senderSession,
        own: false,
        sentAt: new Date().toISOString(),
        body,
      };
      store(message);
      emit("messageReceived", { message });
      const snap = requireSnapshot();
      const followed =
        conversation.kind === "channel" && snap.followedChannelIds.includes(conversation.channelId);
      if (followed || conversation.kind !== "channel") {
        const key = conversationKey(conversation);
        const current = snap.unread.find((u) => conversationKey(u.conversation) === key);
        setUnread(conversation, (current?.count ?? 0) + 1);
      }
      return message;
    },
    loseConnection: () => {
      const snap = requireSnapshot();
      setState({
        state: "reconnecting",
        serverId: snap.serverId,
        attempt: 1,
        maxAttempts: 10,
        nextRetrySecs: 2,
      });
    },
    completeReconnect: () => {
      if (state.state !== "reconnecting") return;
      const snap = requireSnapshot();
      setState({ state: "connected", serverId: snap.serverId });
      emit("serverSynced", { snapshot: snap });
    },
    setTalking: (session, talking) => {
      emit("talkStateChanged", { session, talking });
    },
    setConnectionStats: (stats) => {
      connectionStats = { ...connectionStats, ...stats };
    },
    offerRejoin: (serverId, channelPath) => {
      rejoinOffers.set(serverId, channelPath);
    },
  };

  return { ...guarded, kind: "mock", on, mock };
}
