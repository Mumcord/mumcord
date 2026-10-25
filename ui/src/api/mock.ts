/**
 * In-memory engine for building and testing the UI before the real engine exists.
 * It follows the contract in docs/engine-api.md closely enough for UI flows, not the
 * Mumble protocol: no network, no audio.
 */
import type { MessageNotice, NotifyOutcome } from "../bindings/commands";
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
  InlineImage,
  LinkPreview,
  MumbleLink,
  OverlaySettings,
  OverlayState,
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
import { STRESS_SELF_CHANNEL_PATH, stressChannels, stressUsers } from "./mockStress";

export interface MockOptions {
  /**
   * "fresh" simulates the first start (no identity, no servers, wizard pending); "demo" fills
   * the "ready" state with richer fictional data for screenshots, "demoFresh" is the first start
   * with demo behaviour. "stress" is "demo" on a big server (~300 channels, 1000 people) for
   * scale measurements; it also exposes the API as `window.__mumcordMock` for the measuring
   * script. Defaults to `VITE_MUMCORD_MOCK_SEED`, then "ready".
   */
  seed?: "fresh" | "ready" | "demo" | "demoFresh" | "stress";
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
  /** Whether the main window counts as focused for message notifications (default true). */
  setWindowFocused(focused: boolean): void;
  /** System notifications `notifyMessage` "showed", oldest first. */
  desktopNotices(): readonly MessageNotice[];
  /** Sounds `notifyMessage` played for incoming messages, oldest first (QA.41). */
  messageSounds(): readonly ("message" | "mention")[];
}

export type MockApi = MumcordApi & { readonly mock: MockControls };

const SELF_SESSION = 1;
const MOCK_FINGERPRINT = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99";

const DEFAULT_SETTINGS: Settings = {
  wizardCompleted: false,
  theme: "system",
  language: null,
  talkMode: "voiceActivation",
  voiceActivation: "automatic",
  voiceThresholdDbfs: -40,
  pttKey: null,
  allowMouseButtons: false,
  pttCompatibilityMode: false,
  inputDevice: null,
  outputDevice: null,
  noiseSuppression: true,
  echoCancellation: true,
  autoGain: true,
  musicQuality: false,
  sounds: {
    joinLeave: true,
    selfMute: true,
    selfDeaf: true,
    messages: true,
    mention: true,
    mentionNotify: true,
    voiceJoin: true,
    hangUp: true,
  },
  closeBehaviour: "ask",
  chatDisplay: "cozy",
  notifications: "both",
  logLevel: "info",
  sendDefaultAvatar: true,
  autostart: false,
  startMinimized: true,
  connectMuted: true,
  linkPreviews: true,
  previewCacheDays: 7,
  historyMaxAgeDays: null,
  nowPlaying: false,
  nowGaming: false,
};

const DEFAULT_OVERLAY: OverlaySettings = {
  enabled: false,
  showWholeChannel: false,
  showChannelName: true,
  opacity: 90,
  corner: "topLeft",
  customPosition: null,
  width: 240,
  height: 320,
};

/** Offline stand-in for the engine's link previews: a YouTube player card or a page card. */
function mockLinkPreview(url: string): LinkPreview | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  const youtube = /^(?:www\.)?youtu(?:\.be|be\.com)$/.test(parsed.hostname)
    ? (parsed.searchParams.get("v") ?? parsed.pathname.split("/").pop() ?? "")
    : "";
  if (/^[\w-]{11}$/.test(youtube)) {
    return {
      url,
      siteName: "YouTube",
      title: "Demo video",
      description: null,
      image: null,
      video: {
        site: "youtube",
        playerUrl: `https://www.youtube-nocookie.com/embed/${youtube}?autoplay=1`,
        channel: "Demo channel",
      },
      isImage: false,
    };
  }
  return {
    url,
    siteName: parsed.hostname,
    title: `Preview of ${parsed.hostname}`,
    description: "Link previews come from the engine; this is mock data.",
    image: null,
    video: null,
    isImage: false,
  };
}

/** Offline stand-in for the engine's inline images: the same small picture for every http(s) URL. */
function mockInlineImage(url: string): InlineImage | null {
  if (!/^https?:\/\//i.test(url)) return null;
  return { url, src: MOCK_DEFAULT_AVATAR, width: 320, height: 180, animated: false };
}

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
      ...noDescription,
    },
    {
      id: 1,
      parentId: 0,
      name: "Lobby",
      position: 0,
      permissions: { ...allow },
      description: MOCK_LOBBY_DESCRIPTION,
      hasDescription: true,
    },
    // Like a long description on a real server: only announced, fetched on request.
    {
      id: 2,
      parentId: 0,
      name: "Raid",
      position: 1,
      permissions: { ...allow },
      description: null,
      hasDescription: true,
    },
    {
      id: 3,
      parentId: 0,
      name: "AFK",
      position: 2,
      permissions: { ...allow, textMessage: false },
      ...noDescription,
    },
  ];
}

/** Fictional demo data for screenshots (seed "demo"); example.org addresses only. */
const DEMO_SERVERS: [string, string][] = [
  ["Night Owls", "voice.example.org"],
  ["Studio Crew", "studio.example.org"],
  ["Weekend League", "league.example.net"],
  ["Book Club", "books.example.com"],
];

function demoChannels(): ChannelInfo[] {
  const channel = (id: number, parentId: number, name: string, position: number) => ({
    id,
    parentId,
    name,
    position,
    permissions: { ...allow },
    description: DEMO_DESCRIPTIONS.get(id) ?? null,
    hasDescription: DEMO_DESCRIPTIONS.has(id),
  });
  return [
    { ...channel(0, 0, "Night Owls", 0), parentId: null },
    channel(1, 0, "Lobby", 0),
    channel(2, 0, "Gaming", 1),
    channel(4, 2, "Squad A", 0),
    channel(5, 2, "Squad B", 1),
    channel(6, 0, "Music Room", 2),
    channel(7, 0, "Study Hall", 3),
    { ...channel(3, 0, "AFK", 4), permissions: { ...allow, textMessage: false } },
  ];
}

/** Abstract head-and-shoulders picture on a gradient, standing in for an uploaded avatar. */
function demoAvatar(from: string, to: string): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs>` +
    `<rect width="64" height="64" fill="url(#g)"/>` +
    `<circle cx="32" cy="25" r="11" fill="#fff" fill-opacity=".85"/>` +
    `<path d="M10 64c2-14 11-21 22-21s20 7 22 21z" fill="#fff" fill-opacity=".85"/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const DEMO_AVATARS = new Map<number, string>([
  [SELF_SESSION, demoAvatar("#14b8a6", "#3b82f6")],
  [2, demoAvatar("#f59e0b", "#db2777")],
  [5, demoAvatar("#6366f1", "#06b6d4")],
  [7, demoAvatar("#8b5cf6", "#ec4899")],
  [9, demoAvatar("#0ea5e9", "#22c55e")],
  [11, demoAvatar("#f43f5e", "#f97316")],
  [13, demoAvatar("#10b981", "#84cc16")],
  [15, demoAvatar("#a855f7", "#3b82f6")],
]);

function demoHistory(): ChatMessage[] {
  const lines: [number, string, string, number][] = [
    [2, "Maya", "Evening everyone! Anyone up for a round later?", 34],
    [5, "Jonas", "Count me in. Give me twenty minutes to finish dinner.", 31],
    [7, "Priya", "I'm in too. Same squads as last week?", 29],
    [SELF_SESSION, "Zoe", "Sounds good. I'll open <b>Squad A</b> once we're all here.", 27],
    [
      2,
      "Maya",
      `Perfect. Here are the notes from last time: <a href="https://example.org/notes">example.org/notes</a>`,
      12,
    ],
    [2, "Maya", "Also: new mics sound great, thanks for the tips!", 11],
    [5, "Jonas", "Back. Who's hosting?", 2],
  ];
  return lines.map(([session, name, body, minutesAgo], index) => ({
    id: `demo-${String(index)}`,
    conversation: { kind: "channel", channelId: 1 },
    senderName: name,
    senderCertHash: `${name.toLowerCase()}hash`,
    senderSession: session,
    own: session === SELF_SESSION,
    sentAt: new Date(Date.now() - minutesAgo * 60_000).toISOString(),
    body,
    dismissedPreviews: [],
  }));
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
    suppressed: false,
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
  const seed = options.seed ?? import.meta.env.VITE_MUMCORD_MOCK_SEED ?? "ready";
  const fresh = seed === "fresh" || seed === "demoFresh";
  const stress = seed === "stress";
  const demo = seed === "demo" || seed === "demoFresh" || stress;
  const serverVersion = options.serverVersion ?? "1.5.735";
  const maxListeners = options.maxListeners ?? 2;
  const connectDelayMs = options.connectDelayMs ?? 0;

  const listeners = new Map<EventName, Set<(payload: never) => void>>();
  const failures = new Map<CommandName, ApiError>();
  const certPrompts = new Map<number, string | null>();

  let identity: Identity | null = fresh ? null : { name: "Zoe", certHash: "selfhash" };
  let settings: Settings = { ...DEFAULT_SETTINGS, wizardCompleted: !fresh };
  let defaultAvatar: string | null = demo ? demoAvatar("#14b8a6", "#3b82f6") : null;
  let servers: SavedServer[] = fresh
    ? []
    : demo
      ? DEMO_SERVERS.map(([label, address], index) => ({
          id: index + 1,
          label,
          address,
          port: 64738,
          username: "Zoe",
          hasPassword: false,
          saveHistory: true,
        }))
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
  let nextServerId = servers.length + 1;
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
  let windowFocused = true;
  let overlay: OverlayState = {
    settings: DEFAULT_OVERLAY,
    moveMode: false,
    onTopSupported: true,
    positionSupported: true,
    explanationKey: null,
    language: null,
  };
  const setOverlay = (next: OverlayState) => {
    overlay = next;
    emit("overlayChanged", { state: overlay });
    return Promise.resolve(overlay);
  };
  const desktopNotices: MessageNotice[] = [];
  const messageSounds: ("message" | "mention")[] = [];
  const rejoinOffers = new Map<number, string[]>();
  // History per server id, keyed by conversation.
  const history = new Map<number, Map<string, ChatMessage[]>>(
    demo
      ? [[1, new Map([[conversationKey({ kind: "channel", channelId: 1 }), demoHistory()]])]]
      : [],
  );

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

  /** Pretend size of the preview cache, freed by `previewCacheClear`. */
  let cachedPreviewBytes = 1_468_006;
  // GIF favorites, most recently used first.
  let gifFavorites: string[] = [];

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
    const channels = stress
      ? stressChannels(requireServer(serverId).label)
      : demo
        ? demoChannels()
        : seedChannels();
    const target =
      (channelPath && findChannelByPath(channels, channelPath)) ??
      (stress ? findChannelByPath(channels, STRESS_SELF_CHANNEL_PATH) : undefined) ??
      channels[1];
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
      welcomeText: demo
        ? `Welcome to <b>${requireServer(serverId).label}</b>. Be kind and have fun!`
        : "Welcome to the <b>mock</b> server.",
      defaultChannelId: target?.id ?? null,
      maxMessageLength: 5000,
      maxImageMessageLength: 131072,
      allowHtml: true,
    };
    return {
      serverId,
      selfSession: SELF_SESSION,
      info,
      channels,
      users: stress
        ? stressUsers(self, channels)
        : demo
          ? demoUsers(self)
          : [self, anna, person(3, "Ben", 1, null), carla],
      followedChannelIds: [],
      unread: [],
      readOnly,
      rejoinChannelId: offer?.id ?? null,
    };
  }

  function demoUsers(self: User): User[] {
    const withAvatar = (user: User): User => ({
      ...user,
      hasAvatar: true,
      avatarHash: `demo-${String(user.session)}`,
    });
    return [
      withAvatar(self),
      withAvatar({ ...person(2, "Maya", 1, "mayahash"), hasComment: true }),
      withAvatar(person(5, "Jonas", 1, "jonashash")),
      withAvatar(person(7, "Priya", 1, "priyahash")),
      { ...person(3, "Ben", 1, null), selfMute: true },
      { ...person(4, "Carla", 7, "carlahash"), listeningChannelIds: [1] },
      person(6, "Leo", 4, "leohash"),
      withAvatar(person(9, "Sam", 4, "samhash")),
      person(8, "Elif", 6, "elifhash"),
      withAvatar(person(11, "Nora", 6, "norahash")),
      person(12, "Tim", 6, "timhash"),
      withAvatar(person(13, "Ana", 6, "anahash")),
      person(14, "Kai", 6, "kaihash"),
      withAvatar(person(15, "Lena", 6, "lenahash")),
      { ...person(10, "Noah", 3, "noahhash"), selfMute: true, selfDeaf: true },
    ];
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
      dismissedPreviews: [],
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

    channelDescriptionRequest: (channelId) => {
      const channel = requireChannel(channelId);
      const text = MOCK_FETCHED_DESCRIPTIONS.get(channelId);
      if (channel.description === null && channel.hasDescription && text !== undefined) {
        const updated = { ...channel, description: text };
        const snap = requireSnapshot();
        snap.channels = snap.channels.map((c) => (c.id === channelId ? updated : c));
        emit("channelUpserted", { channel: updated });
      }
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
          ? ((stress && session !== SELF_SESSION
              ? demoAvatar(`hsl(${String((session * 47) % 360)} 70% 55%)`, "#1e293b")
              : undefined) ??
            (demo ? DEMO_AVATARS.get(session) : undefined) ??
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAd0lEQVR42u3UwQ2AMAwDwE7HGt2DCRmDTdw/KIlFHIRQrHzrE5Qwzn2WzmiggQZoALcoARjRAHCTBUDkOQA6H35FAuCYmz8+EB7/DYCCXJ+gov1dQGiYl1wOSIz4MxW2m3ugavcWTdIeb3Kmmv1VOAxzlgIyUw4sffQx8QiPKKAAAAAASUVORK5CYII=")
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
    channelFollowAll: () => {
      const snap = requireSnapshot();
      if (!snap.info.followSupported) fail("serverTooOld", "following needs server 1.4 or newer");
      const own = snap.users.find((u) => u.session === SELF_SESSION)?.channelId;
      const summary = { followed: 0, limitReached: 0, notAllowed: 0, noAnswer: 0 };
      const followed = [...snap.followedChannelIds];
      for (const channel of snap.channels) {
        if (channel.id === own || followed.includes(channel.id)) continue;
        if (!channel.permissions.listen) summary.notAllowed += 1;
        else if (followed.length >= maxListeners) summary.limitReached += 1;
        else {
          followed.push(channel.id);
          summary.followed += 1;
        }
      }
      if (summary.followed > 0) {
        snap.followedChannelIds = followed;
        updateUser(SELF_SESSION, { listeningChannelIds: followed });
        emit("followedChannelsChanged", { channelIds: followed });
      }
      return Promise.resolve(summary);
    },
    channelUnfollowAll: () => {
      const snap = requireSnapshot();
      snap.followedChannelIds = [];
      updateUser(SELF_SESSION, { listeningChannelIds: [] });
      emit("followedChannelsChanged", { channelIds: [] });
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

    // links
    linkPreview: (url) => Promise.resolve(settings.linkPreviews ? mockLinkPreview(url) : null),
    inlineImage: (url) => Promise.resolve(settings.linkPreviews ? mockInlineImage(url) : null),
    previewDismiss: (messageId, url) => {
      for (const conversations of history.values()) {
        for (const messages of conversations.values()) {
          const message = messages.find((m) => m.id === messageId);
          if (message && !message.dismissedPreviews.includes(url))
            message.dismissedPreviews.push(url);
        }
      }
      return Promise.resolve(null);
    },
    gifFavorites: () => Promise.resolve([...gifFavorites]),
    gifFavoriteAdd: (url) => {
      if (!/^https?:\/\//i.test(url)) fail("invalidInput", "favorites need an http(s) link");
      gifFavorites = [url, ...gifFavorites.filter((u) => u !== url)].slice(0, 200);
      return Promise.resolve(null);
    },
    gifFavoriteRemove: (url) => {
      gifFavorites = gifFavorites.filter((u) => u !== url);
      return Promise.resolve(null);
    },
    gifFavoriteUse: (url) => {
      if (gifFavorites.includes(url))
        gifFavorites = [url, ...gifFavorites.filter((u) => u !== url)];
      return Promise.resolve(null);
    },
    saveImage: () => Promise.resolve(null),
    previewCacheClear: () => {
      const freed = cachedPreviewBytes;
      cachedPreviewBytes = 0;
      return Promise.resolve(freed);
    },

    // now playing: no media player in the mock; other people's tracks come from `mock.emit`.
    nowPlayingStatus: () => Promise.resolve({ supported: true, track: null }),
    nowPlayingCover: () => Promise.resolve(null),

    // now gaming: no game detection in the mock; other people's games come from `mock.emit`.
    nowGamingStatus: () => Promise.resolve({ supported: true, game: null }),
    nowGamingIcon: () => Promise.resolve(null),
    nowGamingJoin: () => Promise.resolve(null),

    // overlay: no window in the mock; the state round-trips like the engine's.
    overlayState: () => Promise.resolve(overlay),
    overlayUpdate: (next) =>
      setOverlay({
        ...overlay,
        settings: next,
        moveMode: next.enabled && overlay.moveMode,
      }),
    overlaySetEnabled: (enabled) =>
      setOverlay({
        ...overlay,
        settings: { ...overlay.settings, enabled },
        moveMode: enabled && overlay.moveMode,
      }),
    overlayMoveMode: (active) =>
      setOverlay({ ...overlay, moveMode: overlay.settings.enabled && active }),

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
    // Mouse buttons come back unchanged (no desktop dialog), keyboard keys as "F13" (the demo
    // keeps the pressed key so screenshots show a realistic shortcut).
    pttCaptureKey: (key) => Promise.resolve(demo || /^Mouse(Middle|4|5)$/.test(key) ? key : "F13"),
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
      Promise.resolve(
        demo
          ? {
              version: "0.1.0",
              os: "Linux x86_64",
              thirdPartyNotices:
                "Mumble\nCopyright (c) The Mumble Developers\nBSD-3-Clause\n\nTauri - Apache-2.0 OR MIT\n\nReact - MIT",
            }
          : {
              version: "0.0.0-mock",
              os: "mock",
              thirdPartyNotices:
                "Mumble\nCopyright (c) The Mumble Developers\nBSD-3-Clause\n\nSome library - Apache-2.0",
            },
      ),

    // message notifications: same decision as the engine (src-tauri/src/notifications.rs)
    notifyMessage: (notice) => {
      const mode = settings.notifications;
      // Same choice as the engine: the ping replaces the message sound (gating by chat omitted).
      if (notice.mention && settings.sounds.mention) messageSounds.push("mention");
      else if (settings.sounds.messages) messageSounds.push("message");
      let outcome: NotifyOutcome;
      if (windowFocused && notice.onScreen) outcome = "none";
      else if (mode === "off") outcome = "none";
      else if (mode === "desktop") outcome = "desktop";
      else if (windowFocused) outcome = "toast";
      else outcome = mode === "both" ? "desktop" : "none";
      if (outcome === "desktop") desktopNotices.push(notice);
      return Promise.resolve(outcome);
    },
    notificationDismiss: () => Promise.resolve(null),
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
        dismissedPreviews: [],
      };
      store(message);
      emit("messageReceived", { message });
      const snap = requireSnapshot();
      const followed =
        conversation.kind === "channel" && snap.followedChannelIds.includes(conversation.channelId);
      const inOwnChannel =
        conversation.kind === "channel" && conversation.channelId === selfUser().channelId;
      if (followed || inOwnChannel || conversation.kind !== "channel") {
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
    setWindowFocused: (focused) => {
      windowFocused = focused;
    },
    desktopNotices: () => desktopNotices,
    messageSounds: () => messageSounds,
  };

  const api: MockApi = { ...guarded, kind: "mock", on, mock };
  if (stress) window.__mumcordMock = api;
  return api;
}

/** Pinned channel info (FR-1.48b): demo descriptions and the text behind a hash-only one. */
const noDescription = { description: null, hasDescription: false };

const MOCK_LOBBY_DESCRIPTION = "<p><b>Welcome to the Lobby!</b> Say hi and pick a channel.</p>";

const MOCK_FETCHED_DESCRIPTIONS = new Map<number, string>([
  [
    2,
    "<p><b>Raid rules</b></p><ul><li>Be on time: we start at 20:00 sharp.</li>" +
      "<li>Bring potions and food.</li><li>Push-to-talk during boss fights.</li>" +
      "<li>Loot is shared by need before greed.</li><li>Have fun and be kind.</li></ul>" +
      '<p>Guide: <a href="https://example.org/raid">example.org/raid</a></p>',
  ],
]);

const DEMO_DESCRIPTIONS = new Map<number, string>([
  [1, "<p>Hang out here and say hi. New? Check <b>#Gaming</b> for squads.</p>"],
  [
    2,
    "<p><b>Gaming</b>: find your squad below.</p><p>Push-to-talk in Squad channels, " +
      "and keep it friendly. Event nights are on Fridays at 20:00.</p>",
  ],
]);
