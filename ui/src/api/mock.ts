/**
 * In-memory engine for building and testing the UI before the real engine exists.
 * It follows the contract in docs/engine-api.md closely enough for UI flows, not the
 * Mumble protocol: no network, no audio.
 */
import { mockPublicServerPing, mockPublicServers } from "./mockPublicServers";
import type {
  DndState,
  InspectorView,
  LinkOutcome,
  MessageNotice,
  MumbleImportDone,
  MumbleImportFound,
  MumbleLinkStats,
  NotifyOutcome,
  SystemDnd,
  WhisperSetup,
  WhisperState,
  WhisperTarget,
  WindowLook,
} from "../bindings/commands";
import { parseMumbleLink } from "../features/chat/mumbleLink";
import type {
  AcceptedConversation,
  BlockedUser,
  ActiveAudioProfile,
  PmConversationPrefs,
  PrivateConversation,
  ApiError,
  AudioDevices,
  ChannelAudioProfile,
  ChannelInfo,
  ChatMessage,
  ConnectionState,
  ConnectionStats,
  LatencyEstimate,
  Conversation,
  ErrorCode,
  Friend,
  HistoryScope,
  Identity,
  AudioInfo,
  InlineImage,
  LinkPreview,
  MusicEmbed,
  MessageRequest,
  MumbleLink,
  GameKey,
  GameOverlayEntry,
  GamePlugin,
  InGameOverlayState,
  PmRequests,
  HotkeyConflict,
  HotkeysStatus,
  InAppShortcut,
  PttAvailability,
  PttMouseStatus,
  RelayInfo,
  RelayUnavailable,
  SavedServer,
  ServerInfo,
  ServerInput,
  ScreenShare,
  ShareStartBlock,
  ServerSnapshot,
  Settings,
  User,
  WatchParty,
} from "../bindings/commands";
import {
  EngineError,
  type CommandName,
  type EngineCommands,
  type EventName,
  type EventPayloads,
  type MumcordApi,
  type ServerEventName,
  type Unsubscribe,
} from "./client";
import { STRESS_SELF_CHANNEL_PATH, stressChannels, stressUsers } from "./mockStress";
import { createMockInspector } from "./mockInspector";
import { createMockPopouts } from "./mockPopout";
import { createMockUpdater } from "./mockUpdater";

export interface MockOptions {
  /**
   * "fresh" simulates the first start (no identity, no servers, wizard pending); "demo" fills
   * the "ready" state with richer fictional data for screenshots, "demoFresh" is the first start
   * with demo behaviour. "stress" is "demo" on a big server (~300 channels, 1000 people) for
   * scale measurements. The demo seeds expose the API as `window.__mumcordMock` for the measuring
   * and screenshot scripts. Defaults to `VITE_MUMCORD_MOCK_SEED`, then "ready".
   */
  seed?: "fresh" | "ready" | "demo" | "demoFresh" | "stress";
  /** Simulated server: protocol version and listener limit. */
  serverVersion?: string;
  maxListeners?: number;
  /** Delay before connection events fire; 0 emits them synchronously (handy in tests). */
  connectDelayMs?: number;
  /**
   * What the import from the official Mumble client finds (issue #66); `null` = not installed.
   * Defaults to fictional data in the demo seeds and to `null` otherwise.
   */
  mumble?: MumbleImportFound | null;
}

/** Test and dev controls on top of the normal API. */
export interface MockControls {
  emit<K extends EventName>(event: K, payload: EventPayloads[K]): void;
  /** Makes the next call of `command` fail with the given error. */
  failNext(command: CommandName, code: ErrorCode, message?: string): void;
  /**
   * `serverId` refuses every connect that does not bring `password` (a one-off one, else the one
   * last saved for the server), like a server with a password set.
   */
  requirePassword(serverId: number, password: string): void;
  /** The next connect to `serverId` asks for certificate trust first. */
  requireCertTrust(serverId: number, previousFingerprint?: string): void;
  /** Simulates an incoming message from another person. */
  receiveMessage(conversation: Conversation, body: string, senderSession: number): ChatMessage;
  /**
   * What the next `historySyncRequest` for `channelId` finds in `holderName`'s history
   * (FR-1.60); without it, a sync finds nothing.
   */
  offerHistory(
    channelId: number,
    holderName: string,
    lines: readonly { senderName: string; body: string; sentAt: string }[],
  ): void;
  /** Simulates a dropped connection; the mock enters `reconnecting`. */
  loseConnection(): void;
  /** Ends a reconnect attempt successfully. */
  completeReconnect(): void;
  setTalking(session: number, talking: boolean): void;
  /** Simulates a person starting (`kind`) or stopping (`null`) to whisper or shout to the user. */
  setIncomingWhisper(session: number, kind: "whisper" | "shout" | null): void;
  /** Makes whisper keys unavailable, like a native Wayland session. */
  setWhisperKeysUnavailable(unavailable: boolean): void;
  /** Changes what `connectionStats` reports while connected (e.g. a lower bitrate). */
  setConnectionStats(stats: Partial<ConnectionStats>): void;
  /**
   * Simulates that the app last quit while joined to `channelPath` on `serverId`: the next fresh
   * connect to that server offers to rejoin it (QA.9).
   */
  offerRejoin(serverId: number, channelPath: string[]): void;
  /** Whether the main window counts as focused for message notifications (default true). */
  setWindowFocused(focused: boolean): void;
  /** Simulates the desktop's own Do Not Disturb (FR-3.112). */
  setSystemDnd(system: SystemDnd): void;
  /** System notifications `notifyMessage` "showed", oldest first. */
  desktopNotices(): readonly MessageNotice[];
  /** Sounds `notifyMessage` played for incoming messages, oldest first (QA.41). */
  messageSounds(): readonly ("message" | "mention")[];
  /** Simulates a person leaving the server. */
  removeUser(session: number): void;
  /** Simulates a person joining the server (or changing, if the session exists). */
  upsertUser(user: User): void;
  /** Takes away the Enter permission of a channel; `MOCK_CHANNEL_PASSWORD` opens it (FR-2.78). */
  lockChannel(channelId: number): void;
  /** Changes a channel as a server update would, e.g. its user limit or permissions. */
  updateChannel(channelId: number, patch: Partial<Omit<ChannelInfo, "id">>): void;
  /** Renames a channel, like an admin on the server would. */
  renameChannel(channelId: number, name: string): void;
  /** Sets a channel's user limit, like an admin on the server would. */
  limitChannel(channelId: number, maxUsers: number): void;
  /** Stores a channel password without any details, like one saved before issue #146. */
  addBareChannelPassword(serverId: number, password: string): void;
  /**
   * Holds a chat request (with `note`) or a message request (with `messages`) from `senderSession`
   * on the focused server, like the engine does under "Friends only" (issue #141).
   */
  receivePmRequest(
    senderSession: number,
    request: Pick<MessageRequest, "chatRequest" | "note"> & { messages?: string[] },
  ): MessageRequest;
  /**
   * Which connected server the controls above act on (e.g. `receiveMessage`); by default the
   * server that connected last.
   */
  focusServer(serverId: number): void;
}

export type MockApi = MumcordApi & {
  readonly mock: MockControls;
  /**
   * Fictional media data for the demo seeds (issue #163). The mock has no peer, so the media views
   * show this placeholder picture instead of a stream; it lets the views be seen and screenshotted.
   */
  readonly mediaDemo: MockMediaDemo;
};

export interface MockMediaDemo {
  readonly picture: string;
  readonly share: ScreenShare | null;
  readonly party: WatchParty | null;
  readonly quality: {
    width: number;
    height: number;
    fps: number;
    kbps: number;
    viewers: { session: number; fps: number; struggling: boolean }[];
  } | null;
}

interface MockSession {
  state: ConnectionState;
  /** `null` until the session is synced. */
  snapshot: ServerSnapshot | null;
}

const SELF_SESSION = 1;

const NO_RELAY: RelayInfo = { bots: [], host: null, status: { kind: "none" }, firstSeen: null };

/** A relay announced by a bot and trusted since the first visit (spec 04, FR-4.20). */
const MOCK_RELAY: RelayInfo = {
  bots: ["RelayBot"],
  host: "relay.example.org",
  status: { kind: "available" },
  firstSeen: "2026-09-01T18:00:00Z",
};

/** Per-server switches of a server that was just added. */
const NEW_SERVER_FLAGS = {
  alwaysConnectAtStart: false,
  wasVoiceServerAtQuit: false,
  notificationsMuted: false,
  groupMentionsMuted: false,
  themeId: null,
  identityId: null,
  p2pModeOverride: null,
} satisfies Partial<SavedServer>;
/** The one channel password the mock accepts (FR-2.78). */
export const MOCK_CHANNEL_PASSWORD = "secret";

/** A stored channel password with what the engine remembers about it (issue #146). */
interface MockChannelToken {
  token: string;
  label: string | null;
  channels: { channelId: number; path: string[] }[];
  lastUsedAt: string | null;
}

/** Channel path below the root, like the engine's. */
function mockChannelPath(channels: readonly ChannelInfo[], id: number): string[] {
  const path: string[] = [];
  let current = channels.find((c) => c.id === id);
  while (current?.parentId != null) {
    path.unshift(current.name);
    const parentId = current.parentId;
    current = channels.find((c) => c.id === parentId);
  }
  return path;
}
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
  globalHotkeys: true,
  hotkeys: [],
  inputDevice: null,
  outputDevice: null,
  noiseSuppression: true,
  noiseSuppressionBackend: "standard",
  echoCancellation: true,
  autoGain: true,
  inputGainPercent: 100,
  outputVolumePercent: 100,
  musicQuality: false,
  lowLatency: false,
  videoDucking: true,
  videoDuckingPercent: 30,
  bitrateKbps: 96,
  sounds: {
    joinLeave: true,
    selfMute: true,
    selfDeaf: true,
    messages: true,
    mention: true,
    mentionNotify: true,
    voiceJoin: true,
    hangUp: true,
    partyStarted: true,
    partyJoined: true,
    partyLeft: true,
    shareStarted: true,
    shareStopped: true,
    viewerStarted: true,
    viewerStopped: true,
    kicked: true,
    banned: true,
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
  historySharing: true,
  keyboardHints: true,
  announcements: true,
  skipLinks: true,
  offerFullImages: true,
  loadFullImages: true,
  positional: { enabled: false, minDistance: 1, maxDistance: 15 },
  maxConnectedServers: 5,
  appThemeId: "mumcord-dark",
  customThemes: [],
  visualEffects: true,
  hideEmptyChannels: false,
  talkingFirst: false,
  showWeakConnectionIcon: true,
  allowDirectConnections: "unset",
  p2pMode: "directRelay",
  updateCheck: "unset",
  updateAutoDownload: false,
  pluginsUpdates: "auto",
  pluginsAllOff: false,
  pluginsBadgePlugin: null,
  pluginsMirrorUrl: null,
  developerMode: false,
  updateIntervalHours: 12,
  updateSkipped: null,
  appimageMenuEntry: "unset",
  windowOverlayNoticeShown: false,
  whatsNewAuto: true,
  whatsNewSeen: null,
  setupPreset: null,
  privateMessages: {
    rule: "everyone",
    serverRules: [],
    allowCurrentChannel: true,
    allowAccepted: true,
    keepRequests: true,
  },
  blockedMessages: "collapse",
};

/** Fictional titles and covers for the demo party's videos (issue #163). */
const DEMO_VIDEOS: Record<string, { title: string; hue: number }> = {
  dQw4w9WgXcQ: { title: "Northern Lights over a Quiet Lake", hue: 300 },
  tk1a2b3c4d5: { title: "Tide Pools at Dawn", hue: 190 },
  pr7c8d9e0f1: { title: "Desert Light, a Timelapse", hue: 20 },
  pl4a5n6t7e8: { title: "Build a Paper Lantern in 10 Minutes", hue: 250 },
};

/** Offline stand-in for the engine's link previews: a YouTube player card or a page card. */
function mockLinkPreview(url: string, demo: boolean): LinkPreview | null {
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
  const music = mockMusic(parsed);
  if (music !== null) {
    return {
      url,
      siteName: null,
      title: MOCK_MUSIC_TITLES[music.kind] ?? "Midnight Signals",
      description: music.artist === null ? "Lumen Park · Single · 2025" : null,
      image: demoCover(music.service === "appleMusic" ? 20 : 330),
      video: null,
      music,
      isImage: false,
      isAudio: false,
    };
  }
  if (/^[\w-]{11}$/.test(youtube)) {
    const named = demo ? DEMO_VIDEOS[youtube] : undefined;
    return {
      url,
      siteName: "YouTube",
      title: named?.title ?? "Demo video",
      description: null,
      image: named === undefined ? null : demoCover(named.hue),
      video: {
        site: "youtube",
        playerUrl: `https://www.youtube-nocookie.com/embed/${youtube}?autoplay=1`,
        channel: "Demo channel",
      },
      music: null,
      isImage: false,
      isAudio: false,
    };
  }
  if (demo) {
    return {
      url,
      siteName: parsed.hostname,
      title: "Squad notes",
      description: "Fictional notes from last week's session.",
      image: demoPicture(),
      video: null,
      music: null,
      isImage: false,
      isAudio: false,
    };
  }
  return {
    url,
    siteName: parsed.hostname,
    title: `Preview of ${parsed.hostname}`,
    description: "Link previews come from the engine; this is mock data.",
    image: null,
    video: null,
    music: null,
    isImage: false,
    isAudio: false,
  };
}

const MOCK_MUSIC_TITLES: Partial<Record<MusicEmbed["kind"], string>> = {
  album: "Glass Harbour",
  playlist: "Night Drive Mix",
};

/** Offline stand-in for the engine's music link recognition (issue #262), by host only. */
function mockMusic(url: URL): MusicEmbed | null {
  const kind = /\/(album|playlist)\//.test(url.pathname)
    ? url.pathname.includes("/album/")
      ? "album"
      : "playlist"
    : "track";
  const id = url.pathname.split("/").pop() ?? "";
  switch (url.hostname) {
    case "open.spotify.com":
      return {
        service: "spotify",
        kind,
        playerUrl: `https://open.spotify.com/embed/${kind}/${id}`,
        artist: null,
      };
    case "soundcloud.com":
      return {
        service: "soundcloud",
        kind,
        playerUrl: `https://w.soundcloud.com/player/?url=${encodeURIComponent(url.href)}`,
        artist: "Lumen Park",
      };
    case "music.apple.com":
      return { service: "appleMusic", kind, playerUrl: null, artist: "Nora Vale" };
    case "tidal.com":
      return { service: "tidal", kind, playerUrl: null, artist: "Lumen Park" };
    case "tunes.example":
      return { service: "other", kind, playerUrl: null, artist: null };
    default:
      return null;
  }
}

/** A gradient cover with a ring, drawn in the browser; `null` without a canvas (tests). */
function demoCover(hue: number): string | null {
  const canvas = document.createElement("canvas");
  canvas.width = 96;
  canvas.height = 96;
  const context = canvas.getContext("2d");
  if (context === null) return null;
  const gradient = context.createLinearGradient(0, 0, 96, 96);
  gradient.addColorStop(0, `hsl(${String(hue)} 90% 70%)`);
  gradient.addColorStop(1, `hsl(${String(hue + 60)} 80% 55%)`);
  context.fillStyle = gradient;
  context.fillRect(0, 0, 96, 96);
  context.strokeStyle = "rgb(255 255 255 / 75%)";
  context.lineWidth = 4;
  context.beginPath();
  context.arc(48, 48, 22, 0, 2 * Math.PI);
  context.stroke();
  return canvas.toDataURL("image/png");
}

/** Offline stand-in for the engine's audio probe (issue #260); links with "broken" fail. */
function mockAudioInfo(url: string): AudioInfo | null {
  const ext = /\.([a-z0-9]+)$/i.exec(new URL(url).pathname)?.[1]?.toLowerCase();
  if (ext === undefined || url.includes("broken")) return null;
  const formats: Record<string, AudioInfo["format"]> = {
    mp3: "mp3",
    ogg: "ogg",
    opus: "ogg",
    m4a: "mp4",
    aac: "aac",
    wav: "wav",
    flac: "flac",
    webm: "webm",
  };
  const format = formats[ext];
  return format === undefined ? null : { url, format, size: 4_200_000 };
}

/** A few seconds of a soft chord as a WAV `data:` URL, standing in for any audio file. */
function mockAudioFile(): string {
  const rate = 8000;
  const samples = rate * 6;
  const bytes = new Uint8Array(44 + samples * 2);
  const view = new DataView(bytes.buffer);
  const text = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i++) bytes[offset + i] = value.charCodeAt(i);
  };
  text(0, "RIFF");
  view.setUint32(4, 36 + samples * 2, true);
  text(8, "WAVEfmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, samples * 2, true);
  for (let i = 0; i < samples; i++) {
    const t = i / rate;
    const envelope = 0.5 + 0.5 * Math.sin(2 * Math.PI * 0.7 * t);
    const value =
      envelope * (Math.sin(2 * Math.PI * 220 * t) + 0.5 * Math.sin(2 * Math.PI * 330 * t));
    view.setInt16(44 + i * 2, Math.round(value * 6000), true);
  }
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:audio/wav;base64,${btoa(binary)}`;
}

/** Offline stand-in for the engine's inline images: the same small picture for every http(s) URL. */
function mockInlineImage(url: string, demo: boolean): InlineImage | null {
  if (!/^https?:\/\//i.test(url)) return null;
  const src = demo ? DEMO_PHOTO : MOCK_DEFAULT_AVATAR;
  return { url, src, width: 320, height: 180, animated: false };
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

/** The figures measured on a PipeWire machine with an 18 ms ping (issue #156), for both modes. */
function mockLatencyEstimate(lowLatency: boolean): LatencyEstimate {
  return lowLatency
    ? {
        lowLatency,
        packetMs: 10,
        inputBufferUs: 2667,
        encodeUs: 12500,
        networkUs: 9000,
        jitterUs: 10000,
        processingUs: 100,
        outputBufferUs: 13667,
        totalUs: 47934,
        otherModeTotalUs: 107134,
      }
    : {
        lowLatency,
        packetMs: 20,
        inputBufferUs: 10667,
        encodeUs: 26500,
        networkUs: 9000,
        jitterUs: 20000,
        processingUs: 300,
        outputBufferUs: 40667,
        totalUs: 107134,
        otherModeTotalUs: 47934,
      };
}

const DEFAULT_CONNECTION_STATS: ConnectionStats = {
  transport: "udp",
  rttMs: 24.5,
  lossPercent: 0,
  sendBitrateKbps: 72,
  serverMaxBandwidthKbps: 558,
  packetsSent: 0,
  packetsReceived: 0,
  // Fair because of network loss, like the issue #152 mockup.
  audio: {
    dropouts: 6,
    lateFrames: 18,
    concealedFrames: 172,
    jitterBufferMs: 40,
    processingAvgUs: 2100,
    processingMaxUs: 7400,
    realtime: false,
    rating: "fair",
    issue: "network",
    windowSecs: 120,
    recentDropouts: 0,
    recentLateFrames: 2,
    recentConcealedFrames: 25,
    recentReceivedFrames: 1175,
    recentProcessingMaxUs: 4800,
    dropoutsHistory: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    lateFramesHistory: [0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0],
    concealedHistory: [0, 1, 1, 0, 2, 3, 2, 4, 5, 3, 2, 2],
    processingMaxHistoryUs: [
      2900, 3100, 2800, 3300, 3000, 3600, 4800, 3900, 3500, 3200, 3700, 3400,
    ],
  },
  latency: mockLatencyEstimate(false),
};

const allow = {
  enter: true,
  textMessage: true,
  listen: true,
  registerSelf: false,
  speak: true,
  whisper: true,
  write: false,
  moveUsers: false,
};

function seedChannels(): ChannelInfo[] {
  return [
    {
      id: 0,
      parentId: null,
      name: "Root",
      position: 0,
      permissions: { ...allow, registerSelf: true },
      ...noDescription,
      maxUsers: null,
      temporary: false,
      links: [],
      enterRestricted: false,
    },
    {
      id: 1,
      parentId: 0,
      name: "Lobby",
      position: 0,
      permissions: { ...allow },
      description: MOCK_LOBBY_DESCRIPTION,
      hasDescription: true,
      maxUsers: null,
      temporary: false,
      links: [],
      enterRestricted: false,
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
      maxUsers: null,
      temporary: false,
      links: [],
      enterRestricted: false,
    },
    {
      id: 3,
      parentId: 0,
      name: "AFK",
      position: 2,
      permissions: { ...allow, textMessage: false },
      ...noDescription,
      maxUsers: null,
      temporary: false,
      links: [],
      enterRestricted: false,
    },
  ];
}

/** A fictional official Mumble client for screenshots (issue #66); "Book Club" is a duplicate. */
export const DEMO_MUMBLE: MumbleImportFound = {
  format: "json",
  favourites: [
    ["Nordic Gamers", "nordic.example.org", true],
    ["Retro LAN", "retro.example.net", true],
    ["Coffee Club", "coffee.example.com", false],
    ["Pixel Harbor", "pixel.example.org", true],
    ["Silent Orbit", "orbit.example.net", false],
    ["Book Club", "books.example.com", false],
  ].map(([label, host, hasPassword]) => ({
    label: label as string,
    host: host as string,
    port: 64738,
    username: "Zoe",
    hasPassword: hasPassword as boolean,
    duplicate: false,
  })),
  favouritesFailed: false,
  certificate: {
    name: "",
    validUntil: null,
    needsPassword: true,
    alreadyImported: false,
  },
  inputDevice: { id: "pipewire:usb-headset", name: "USB Headset" },
  outputDevice: { id: "pipewire:speakers", name: "Speakers" },
  devicesMissing: false,
  pttKey: "Ctrl+Space",
  pttIssue: null,
  talkMode: "pushToTalk",
  outputVolumePercent: 80,
};

/** Password of the locked certificate in {@link DEMO_MUMBLE}. */
export const DEMO_MUMBLE_CERT_PASSWORD = "mumble";

/** Fictional demo data for screenshots (seed "demo"); example.org addresses only. */
const DEMO_SERVERS: [string, string][] = [
  ["Night Owls", "voice.example.org"],
  ["Studio Crew", "studio.example.org"],
  ["Weekend League", "league.example.net"],
  ["Book Club", "books.example.com"],
];

function demoChannels(serverName: string): ChannelInfo[] {
  const channel = (id: number, parentId: number, name: string, position: number) => ({
    id,
    parentId,
    name,
    position,
    // The demo user moderates, so the tree shows "Move users here…" and dragging (#172).
    permissions: { ...allow, moveUsers: true },
    description: DEMO_DESCRIPTIONS.get(id) ?? null,
    hasDescription: DEMO_DESCRIPTIONS.has(id),
    maxUsers: null,
    temporary: false,
    links: [],
    enterRestricted: false,
  });
  return [
    { ...channel(0, 0, serverName, 0), parentId: null },
    channel(1, 0, "Lobby", 0),
    channel(2, 0, "Gaming", 1),
    // A user limit and a link between the squads, for the channel info screenshots (#170).
    { ...channel(4, 2, "Squad A", 0), maxUsers: 5, links: [5] },
    { ...channel(5, 2, "Squad B", 1), links: [4] },
    // Full: six people in, for the user-limit screenshots (#172).
    { ...channel(6, 0, "Music Room", 2), maxUsers: 6 },
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

/** Abstract landscape (sun over hills) standing in for a page picture or an inline image. */
function demoPicture(): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 180">` +
    `<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">` +
    `<stop offset="0" stop-color="#0f172a"/><stop offset="1" stop-color="#f97316"/></linearGradient></defs>` +
    `<rect width="320" height="180" fill="url(#sky)"/>` +
    `<circle cx="236" cy="72" r="26" fill="#fde68a"/>` +
    `<path d="M0 130 L80 84 L160 128 L230 96 L320 140 V180 H0Z" fill="#1e293b"/>` +
    `<path d="M0 160 L110 132 L210 158 L320 136 V180 H0Z" fill="#0f172a"/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/**
 * The landscape of `demoPicture` as a WebP (640×360): inline images only accept raster pictures,
 * like the engine delivers them.
 */
const DEMO_PHOTO =
  "data:image/webp;base64,UklGRoQOAABXRUJQVlA4IHgOAABwhQCdASqAAmgBPm02mUkkIyKhIriIGIANiWdu/GgzBDS2XI3/df9y/8hl68//0/tWcaeI3Zmu38o5yZ4i1kfoueFfsX7y6g36V9gD+C/wr/Zfz3+79hD+k+gD+X/7P15PQx/+vUA///p++qTvI/9m/5vo1/////9v/0x/of+M/z3b/5m637B/9z1TMdnIjab3sAtLI1/aaPnKBPKd9F+j+wAV+HQO8e86Ogd4950dA7x7zo6B3j3nRzv94UKToDUnQGpOgNSdAak6A1J0BqToDUnQGpOgNSdAak6A1J0BqToDUnQGpOgNSdAak6A1J0BqU6XhRsBQmpbLa86XPSziAKE1LZbXnS56WcQBQmpbLa7b/ZgoLOQy0mHopyrT8lBZyGWkw9FOVafkoLOQy0mBWYeinKtPyUFnIZaTD0U5Vp+Sgs5DLSYeinKtPyUFglTiX8cvYxWxitjFbGK2MVsYrYjnFBhdOKX4G3V9w0MdU5h6Kcq0+gT8lBb/PV/zlf8tL/JR/xz/8Qv+94iyfob2F5pp8x2G1shrvNXA5XjJRXdBSWIWkw9FOVafkoLOQy0mHopyrT0ps1Tn/2R/QkPh3JQ8PrLlmHokADZCBH4/H4/H4/H4/H4/H4/H4/H4/H4/H4/H49ncM7noH92oHi9K/JQWEyELPBrgIeIJJwSUE4nrNBWccfIFEvSq6i4gBFBJqhqmfkn70r8lBZn++sbNzYgYVHXvGScElBZyGWkw69z11CfF+0h+p35rP/SXdGMBuxC0PoEsiuAGz+OPH7enoB7QBBkARVD9XuIWkw9FOVaenhX5b4ez/VwvLnoDm519NQtrpJnwz/8beqMRobIjs7s7YxU17lewq7TmLOQwz5nx7b2Tb+PRTlWn48wQ8Iyb5RuOnsKEI3+8su0elHE2uoQtJh6JAEW4KqliNLi7LDwgkoUmC8YBrsg92oR8V+Kr0pDuzZbclXM5tyUMjQ0IeGUqS8H/Z4Wt7cxnShr7r9J+XPlXI/wpJTMm3nrpKRJjRUwfCvcUFtw5P8Qt9qJ8EySGbaDWMrQiXnQCrb0tLc6CgtpXUCr3XAj/CklHhfVWZWeEyll4jikuDei2dBKDveBOPkkmBaNr6VFQCePI7Lm9do/PS6OI93Mvl5VnOj1lKWJe2wvk8D0tfPkf4UjWzbRnQKZgsnFvJH+FGYp2PTPbX198ds6B8tW6xLzhD0tZVFPV0JTG3UJjbp5RTeEzzap20Ee/S2npHAF8q+tv+Zjr/70ycfI/2TQon305HukNqCICUWZ2Mu2im7x4xng0knuUef/9YObbiS0Thf3+KsVfrXUJjbqEkIZIl2RLwo/jUo4fLC8XMyvuNuoTG3UJjbqExt1CY290mNuoTG3UJjbqExt1CY26hMbdQmNuoTG3UJjbqExt1CY26hMbdQmNuoTEwAD++tJ33qoFjXoIfYRCgAaPRBhqSGVEKStUTE7G6EUsS9zs/juveeCDR6IMNSQyohSVqiYnY3QiliXudn8j4ofuDSupNfIabi2cMLa8b64dqI908Zpu+KCRQtxvzL9qDocRyaFzkbPqgcsyGz6oHLMhs+qByzIbPqvfP01z5nX3SayY1Ud53qBvhWqO871A3wrVHeeHmuPJYRyAAAAAAh387F6FDg2al8IS+uP4oOlpaCsZbRMtomW0TLaFK1r12y4+Oznp/1iuuWlLbn93R+7Xfhk/7KcPYZpCx4qq/lhf6EypMDcm977Ucgjl6Jem0PsrLCA1leCRh3jChMSOFPZQ1umSkAxYhh59pgitYH0NoXJfWuUzVKj761ymapUffWuUzVKj761kjZ4mPSCrWIzCcTl1ufdH5eIuBJ0OfmEY8qlH1uaXGVtApac4p0R2UnU2281ZfPZ2Xd8yZ9inwr5W8Ie2w3BvaqrqOTHJmQpGgHG6ddU/luYD1W+tvWfypizMK46tDcek1nr8iCTwsLZZzD70Fa2scu2504ELxD8eZOrQImIAn7Jg65t9g56V/VMnxQ8Fg2CTQ+qxg0dgchT+/QuaeEBozXuW7KTXgzlJRodx37+sXuE55cOGvgxjaqzi9brpyXYygoDGH/kyGM+daxYiKKsmv3ZZjG+UEFE+Ue6IMdcjY/ajLIycWo/IK5E7vcGpF1ggeDnCO5xEUjWMBVFn9xjyqgnnW1GDIY6d1RjzrcWVCBM2xQi+AHL2hjBFKQ4vak8aTB56whXNs8LPI0CH1nq8Xv2hCniJwm4RTh9k3W4YWgQwsg5PuIRal3YGxtnx27nDT4icSpYQaviMvrQ68RNV72oLEOXAeB7C5cB4HsLlwHgewuXAeB7DEJQU2MaEdfTg+2n59Drw1e7I4bTL8/nCatMZq1ESZjgbSwulrrUtrHZ9A6LRvZvHrJnVfx9EJBEL3TSIVCCz3tQUPVyBckPLQ9Q983hvcaB9WLstR4dNJWubORwYOp6OXAA14Ktifi/6wR60T69LidwbNXVSa3tCcM7MgbigIgk+A+WhuI+PqPSbx3fbNVK/8MPZX113SCKOuvg2Lhs4m/uqLDw+0pIXGX5DVNgiqWOg5S2Zmj9KEtDYyOVVsaSCevL5i/4tOAUDT61V/38F8tGFb4bEDpX3ZF6kYlnmvfANYZvmWGQa+9uk1BzM8RI7mNKgW7ExrSldC/I4J+Sn+v3SNXIpfuqI4ZFkZ4mSGz6KI5dbJkqRF0Bhxj35wgZG7oAsA0F7gQTXXHbYIh+7zZApiazvxtnBBMXGlWwhf4bxt5jhqqOjjSUbST7cAAJWu1+g8fWGCsV6tlYLkRbNKDZeg8xznIIS22WJxqFxRCZ3RpVllzP+rQfK+xpBJG7803AqtRo9GKGcdR/KKDilnFv4FUuPAu/KCo1coLIcMnHXzf+el5Hhssttb55YhmcfWgjEVbhW+J0R+LWv/5sWhkoM1J23koetVNLOHXNEZEI4fabTlyPFcQpNQf0Q0ndMFDqvEcXZVQKdraWqVsLfNycttyCkH75uGXnCiSm0Ak9T2IugCn6yDuAnUq+1/7Y66CjiO0XRp9DAKDftpX5kOE2uDRch7A7BsmX8SUQ9hHfrFozxSE9pLLIR70qYocEgbF2l3vlZxXm32ItVOGOQKDeCnxD2s1BjXQQHOBK0fbCJMZqX68cUNHb8oEdDQUAIJyiHkkyQCpdFlZWI5zlRnZOAn5gN0iHdaZ/n/2VV00p2L7N4+sZN3Jb376NENrCJJ9iRQGf7xkiwasn3rak2fNgWNpKaG0BB5gH+Ts6anQ8iEtR7joPCcXMhT2d7EZZjhW0K71sMLcmBf7N2wfaoTrmnAwgOBcOhG9ipMvco9GNzXN8Y8E4yVHWvNEYUOt5TxEIb4RulWVjfzsQsRGGNFDmF+BVcnCZsHm3IJbs2GRl+pC5CkXiAofHahJOPj/gTAlBH1rdyLkaKfj7gRn0gbymQROSh4UeHw7MzvN+riwLEAAM/IDTxGqjRN+sA938mrKzzE2rc99NVXMnlFMJ7vqfRnqVnCtDnKx4HSREqq/b18p10mMLIAODmS1v/rYZdyFBR3HjvoAq1EhHYTpJnyij0LplLNf36a/8XZ8+Fq7NBVeS10wKMWP9H2JfVjLreBsF1iwB7vAAErJsjMl2N0vjxgFoRyT0Hk+NNtAXpfBQKBgstri9AWLgipGyit7AiX6BUbCuSe8ydI7M2TglJZrSx8/NXdcQFMIZCQQ3n9n7iLiSZM2Q5H9qBqKzo1VKuViml6+3sDi/mnq8615uguuStASuvFGxXLlPkwZyKrn8m2aDdQ8LLn5LeqBT7rIxNHN2I7c8+dQB4Ik+DxDVDGeCOMuom2xk5PmbIqVagAE3sbcKErmn/TlDn1NyZiDKmazNQmq4brAcKVgdNIBTRJ5opZYbWr5hMyO+QTT/z8n3HQe/a/cp/Ii+UBqERc2R1S/3mz6ZGA+iQyYPsFTT2DZjLlbOQ65t5EYFjx0rTzK2A9LUuV4J2pC6mhz8h0k9CxSpBTm2gIADYBgKcAQxh33oH4Yv6oW92AXkNqmOOmClrQrAlIipIgAHfxjjD0fzsAuycQpvw5BT+OwqoP13+sCb7bjvlmKvQYSfmd8LLBI/cOMQ7TYWtm7RiQH03ViYbjT3IXn/E97vDIC1V8GoaGtl1lEtGHGQnpeLE6carTIaUmEoNb0e0/MsO8+SP4v0tmxfsGSJWFLpnWBns9VrUBsM0fN/rXHDiF0eDcFSFVPQ0fAAAJqZafsD4FmubzBCIdRLYvBAItlHFzXotKgEmO5YifvC+/yUKhYSdjOTiMiVLG0iPgn9NSDkrjBNcrIWFLqy2OhbeBigEzBykQ/880svc5HG1e6+weI4k5GvRXy8gigve1+h5CkkWvDQ/WtIf9ZK2OzaFvXjxMQxuq9um59Ns6ZWxJQBQA+jJdRswAAAaGIk3vMtQDwqSDvqPG/6by2RvrqnT9iKPwbYsQslKMDuz3E46WjLqpL4lfE6X38uY/8NFg3xZGJZzPtbDzMxCoIskO8EHlV+w0MnAhyK87xRDpAtUgZwAHwd22RNVLHXrhl90hpEAAAAAJAWb/g14zEyxXPXQ8++waw+p7565QtQ6wwJiD1Zcj4lxhwr0QbeV8aqR7ojyyEp/HddQqSz6E+3FDI7I+9NAffBsD+oWeBqnJc+dcnPxwQOf7aO8hHK9gABPxHOFGYhnarEc4UZbaK5eE36jtrG+p+lCUnMLP4ptEnV67QnvzNh4Yn0fW/lncWvA/WcS9plUkIBoCEbRODumFcA5Wg4rZs8AA4vZq204piaUDwkojnCjMQztViOcKMts7Ec4o0gyq70cQIsqgF+oWcKYAAAqm3xqxuQ1dXDzyxbOopTR3ZQuHAAAAHAd5CGWEDC4Oe4bAdPGrvxaiygGyFAalqbB/MG3tv1AAAXGXU4AAAAAAAAAAAAAAAAAAAAAAA==";

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
    [5, "Jonas", "Photo from the lake trip: https://images.example.org/lake.png", 9],
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
    syncedFrom: null,
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
    isMumcord: false,
    mumcordFeatures: [],
  };
}

/** Mumcord's own shortcuts, as the engine refuses them for global hotkeys. */
const MOCK_IN_APP_SHORTCUTS: Partial<Record<string, InAppShortcut>> = {
  "Ctrl+Shift+M": "memberColumn",
  "Ctrl+Shift+N": "notificationCentre",
  "Ctrl+K": "search",
  "Ctrl+F": "search",
  "Ctrl+W": "closeWindow",
};

/** The engine's canonical key form for the UI's key strings ("Ctrl+KeyA" → "Ctrl+A"). */
function mockHotkey(key: string): string {
  const parts = key.split("+");
  const main = parts.pop() ?? "";
  if (main === "" || /^(Control|Alt|Shift|Meta)(Left|Right)?$/.test(main)) {
    fail("invalidInput", "a key other than a modifier is required");
  }
  const single = /^(?:Key|Digit)(.)$/.exec(main);
  return [...parts, single ? single[1] : main].join("+");
}

function fail(code: ErrorCode, message: string): never {
  throw new EngineError(code, message);
}

/** Like the engine's stubs for commands that are in the contract but not built yet. */
function notImplemented(command: CommandName): never {
  fail("notImplemented", `${command} is not implemented yet`);
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

/** The same rules as `parse_mumble_link` in the engine, through the UI's single parsing module. */
function parseLink(url: string): MumbleLink {
  return parseMumbleLink(url) ?? fail("invalidInput", "not a mumble:// link");
}

/** Length of each mic-test phase (recording, playback) in the mock. */
const MIC_TEST_PHASE_MS = 5000;

export function createMockApi(options: MockOptions = {}): MockApi {
  const seed = options.seed ?? import.meta.env.VITE_MUMCORD_MOCK_SEED ?? "ready";
  const fresh = seed === "fresh" || seed === "demoFresh";
  const stress = seed === "stress";
  const demo = seed === "demo" || seed === "demoFresh" || stress;
  /** What the mock answered for each `mumble://` link in this run (issue #175). */
  const queryAnswers = new Map<string, MumbleLinkStats>();
  const serverVersion = options.serverVersion ?? "1.5.735";
  const maxListeners = options.maxListeners ?? 2;
  const connectDelayMs = options.connectDelayMs ?? 0;

  /**
   * Fictional media data for the demo seeds (issue #163): a share from another person, a watch
   * party and a placeholder picture. The mock has no peer, so it cannot deliver frames; the media
   * views show this picture instead.
   */
  const mediaDemo: MockMediaDemo = demo
    ? {
        picture: DEMO_PHOTO,
        share: {
          session: 7,
          paused: false,
          sound: false,
          soundAvailable: false,
          kind: "window",
          elapsedMs: 126000,
          viewers: [1, 2, 5],
          blocked: null,
          relayReason: null,
        },
        party: {
          channelId: 1,
          videoId: "dQw4w9WgXcQ",
          playing: true,
          positionMs: 431000,
          participants: [1, 2, 5, 7, 9],
          lastAction: { session: 2, command: { kind: "seek", positionMs: 431000 } },
          joined: true,
          currentEntry: 0,
          skippedBy: null,
          host: 2,
          playlist: [
            { id: 1, videoId: "tk1a2b3c4d5", addedBy: 5, addedByName: "Jonas" },
            { id: 2, videoId: "pr7c8d9e0f1", addedBy: 7, addedByName: "Priya" },
            { id: 3, videoId: "pl4a5n6t7e8", addedBy: 9, addedByName: null },
          ],
          positions: [
            { session: 1, positionMs: 431000, ageMs: 900, playing: true, buffering: false },
            { session: 2, positionMs: 431000, ageMs: 700, playing: true, buffering: false },
            { session: 5, positionMs: 431000, ageMs: 400, playing: true, buffering: true },
          ],
          history: [
            { id: 0, videoId: "dQw4w9WgXcQ" },
            { id: 1, videoId: "tk1a2b3c4d5" },
          ],
        },
        quality: {
          width: 1920,
          height: 1080,
          fps: 60,
          kbps: 4230,
          viewers: [
            { session: 1, fps: 60, struggling: false },
            { session: 2, fps: 60, struggling: false },
            { session: 5, fps: 20, struggling: true },
          ],
        },
      }
    : { picture: "", share: null, party: null, quality: null };

  const listeners = new Map<EventName, Set<(payload: never) => void>>();
  /** Private notes by session (FR-1.63); the engine keys them by certificate hash. */
  const notes = new Map<number, string>();
  /** Friends by certificate hash (FR-1.64); online state is derived from the snapshot. */
  const friends = new Map<string, { name: string; addedAt: string; lastSeenAt: string | null }>();
  let shownFriends = "[]";
  /** Held and sent requests per server (issue #141); memory only, dropped on disconnect. */
  const pmRequestsByServer = new Map<number, PmRequests>();
  let nextPmRequestId = 1;
  let acceptedConversations: AcceptedConversation[] = [];
  /** Blocked users of every own identity (issue #140). */
  let blockedUsers: BlockedUser[] = [];
  let nextBlockId = 1;
  let pmPrefs: PmConversationPrefs[] = [];

  function setPmPrefs(
    serverId: number,
    certHash: string,
    change: Partial<Omit<PmConversationPrefs, "serverId" | "certHash">>,
  ): void {
    const same = (p: PmConversationPrefs) => p.serverId === serverId && p.certHash === certHash;
    const old = pmPrefs.find(same) ?? {
      serverId,
      certHash,
      pinned: false,
      mutedUntil: null,
      mutedForever: false,
    };
    const next = { ...old, ...change };
    const kept = next.pinned || next.mutedUntil !== null || next.mutedForever;
    pmPrefs = [...pmPrefs.filter((p) => !same(p)), ...(kept ? [next] : [])];
    emit("pmConversationPrefsChanged", { prefs: pmPrefs });
  }
  const failures = new Map<CommandName, ApiError>();
  // `?mockUpdate` on a demo seed: someone who chose "Notify only" while a new version is out,
  // for trying and photographing the updater.
  const mockUpdate = demo && new URLSearchParams(window.location.search).has("mockUpdate");
  const inspector = createMockInspector();
  const popouts = createMockPopouts((state) => {
    emit("mediaPopoutChanged", { state });
  });
  const updater = createMockUpdater({
    demo,
    startupCheck: mockUpdate,
    settings: () => settings,
    emit: (status, prompt) => {
      emit("updateStatusChanged", { status, prompt });
    },
  });
  let inspectorView: InspectorView | null = null;
  let windowLook: WindowLook | null = null;
  const certPrompts = new Map<number, string | null>();

  // Identities are added one by one; the first one is the default (issue #148).
  let identities: Identity[] = fresh
    ? []
    : [
        {
          id: 1,
          label: "Default",
          name: "Zoe",
          certHash: mockCertHash("zoe"),
          addedAt: "2026-03-12T09:00:00.000Z",
          isDefault: true,
          serverIds: [],
        },
      ];
  let nextIdentityId = identities.length + 1;
  let settings: Settings = {
    ...DEFAULT_SETTINGS,
    wizardCompleted: !fresh,
    ...(mockUpdate ? { updateCheck: "yes" } : {}),
  };
  // The demo shows the targets of the whisper mockup (issue #67); tests start without any.
  let whisperTargets: WhisperTarget[] = demo
    ? [
        {
          id: "demo-gaming",
          name: "Gaming + sub-channels",
          receivers: {
            kind: "channel",
            path: ["Gaming"],
            children: true,
            links: false,
            group: null,
          },
          hotkey: "F9",
        },
        {
          id: "demo-people",
          name: "Maya and Priya",
          receivers: { kind: "people", names: ["Maya", "Priya"] },
          hotkey: "F10",
        },
        {
          id: "demo-music",
          name: "Music Room",
          receivers: {
            kind: "channel",
            path: ["Music Room"],
            children: false,
            links: false,
            group: null,
          },
          hotkey: null,
        },
      ]
    : [];
  let whisperKeysUnavailable = false;
  const whisperSetup = (): WhisperSetup => ({
    targets: whisperTargets,
    keysUnavailable: whisperKeysUnavailable ? "waylandPortal" : null,
    failedKeys: [],
  });
  // One official game plugin as found in a Mumble installation; no game runs in the mock.
  let gamePlugins: GamePlugin[] = [
    {
      id: "gtav",
      name: "Grand Theft Auto V",
      version: "1.0.0",
      enabled: false,
      active: false,
      problem: null,
    },
  ];
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
          ...NEW_SERVER_FLAGS,
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
            ...NEW_SERVER_FLAGS,
          },
        ];
  let nextServerId = servers.length + 1;
  let nextMessageId = 1;
  let linkHandler = false;
  // Like the engine (FR-2.1): one session per connected server, voice on at most one of them.
  const sessions = new Map<number, MockSession>();
  // Server the running command or control addresses; every entry point sets it first.
  let currentServerId = 0;
  // Server the mock controls act on: the last one that connected, or `focusServer`.
  let focusedServerId = 0;
  let voiceServerId: number | null = null;
  let connectionStats: ConnectionStats = { ...DEFAULT_CONNECTION_STATS };
  let micTimer: ReturnType<typeof setInterval> | undefined;
  let micPhaseTimer: ReturnType<typeof setTimeout> | undefined;
  const requiredPasswords = new Map<number, string>();
  const savedPasswords = new Map<number, string>();
  const channelTokens = new Map<number, MockChannelToken[]>();
  // Channel audio profiles (issue #171) by server, then channel id; and the one in effect.
  const channelAudio = new Map<number, Map<number, ChannelAudioProfile>>();
  let activeAudio: ActiveAudioProfile | null = null;
  let pendingConnect: { serverId: number; channelPath: string[] | null } | null = null;
  let muteBeforeDeaf = false;
  // Like the engine (QA.7): the forced read-only state of a fresh connect, and the user's last
  // own mute/deafen choice while joined for voice, which a join restores.
  let ownVoice = { mute: false, deaf: false };
  let windowFocused = true;
  let dnd: DndState = {
    manual: false,
    followSystem: true,
    system: { desktop: "KDE Plasma", on: false },
  };
  const dndActive = () => dnd.manual || (dnd.followSystem && dnd.system.on === true);
  const setDnd = (next: DndState) => {
    dnd = next;
    emit("dndChanged", { state: dnd });
    return Promise.resolve(dnd);
  };
  let ingameOverlay: InGameOverlayState = {
    supported: true,
    explanationKey: null,
    mode: "off",
    openglLaunchOption: null,
    games: [],
    known: [],
    running: [],
  };
  const setIngameOverlay = (next: InGameOverlayState) => {
    ingameOverlay = next;
    emit("inGameOverlayChanged", { state: ingameOverlay });
    return Promise.resolve(ingameOverlay);
  };
  const effectOf = (entry: GameOverlayEntry): InGameOverlayState["games"][number]["effective"] => {
    if (ingameOverlay.mode === "off") return "globalOff";
    if (entry.blocklist && !entry.riskConfirmed) return "blocked";
    if (entry.enabled === false) return "gameOff";
    if (entry.enabled === null && entry.blocklist) return "blocked";
    return "show";
  };
  const sameKey = (a: GameKey, b: GameKey) =>
    a.kind === b.kind &&
    (a.kind === "steam"
      ? a.value === (b as { value: number }).value
      : a.value.toLowerCase() === (b as { value: string }).value.toLowerCase());
  const desktopNotices: MessageNotice[] = [];
  const messageSounds: ("message" | "mention")[] = [];
  const rejoinOffers = new Map<number, string[]>();
  // History per server id, keyed by conversation.
  const history = new Map<number, Map<string, ChatMessage[]>>(
    demo
      ? [[1, new Map([[conversationKey({ kind: "channel", channelId: 1 }), demoHistory()]])]]
      : [],
  );
  // When "Clear history" last ran (issue #119): per server and channel, per server, or for all.
  const clearedAt = new Map<string, string>();
  const clearedKey = (serverId: number | null, channelId: number | null) =>
    `${String(serverId)}:${String(channelId)}`;

  function dispatch<K extends EventName>(event: K, payload: EventPayloads[K]): void {
    const set = listeners.get(event);
    if (!set) return;
    for (const handler of set) (handler as (p: EventPayloads[K]) => void)(payload);
  }

  /** Friends as the engine reports them: online first, then by name. */
  function friendViews(): Friend[] {
    const live = [...sessions.values()].flatMap((s) =>
      s.state.state === "connected" && s.snapshot ? [s.snapshot] : [],
    );
    const views = [...friends].map(([certHash, f]): Friend => {
      let user: User | undefined;
      let server: ServerSnapshot | undefined;
      for (const snap of live) {
        user = snap.users.find((u) => u.certHash === certHash && u.session !== SELF_SESSION);
        server = snap;
        if (user) break;
      }
      return {
        certHash,
        name: user?.name ?? f.name,
        addedAt: f.addedAt,
        lastSeenAt: f.lastSeenAt,
        online:
          user && server
            ? { serverId: server.serverId, session: user.session, channelId: user.channelId }
            : null,
      };
    });
    return views.sort(
      (a, b) =>
        Number(b.online !== null) - Number(a.online !== null) ||
        a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
    );
  }

  /** Like the engine, emits `friendsChanged` after any event that changes the list. */
  function refreshFriends(): void {
    const now = new Date().toISOString();
    const before = new Map(
      (JSON.parse(shownFriends) as Friend[]).map((f) => [f.certHash, f.online !== null]),
    );
    const views = friendViews();
    for (const view of views) {
      const friend = friends.get(view.certHash);
      if (!friend) continue;
      if (before.get(view.certHash) === true && view.online === null) friend.lastSeenAt = now;
      if (view.online !== null) friend.name = view.name;
    }
    const next = JSON.stringify(friendViews());
    if (next === shownFriends) return;
    shownFriends = next;
    dispatch("friendsChanged", { friends: JSON.parse(next) as Friend[] });
  }

  // The first server announces a trusted relay; in the demo the second one's relay changed.
  const relays = new Map<number, RelayInfo>(
    fresh
      ? []
      : [
          [1, MOCK_RELAY],
          ...(demo
            ? [[2, { ...MOCK_RELAY, status: { kind: "untrusted", keptBlocked: false } }] as const]
            : []),
        ],
  );

  function relayOf(serverId: number): RelayInfo {
    return relays.get(serverId) ?? NO_RELAY;
  }

  /** Why the relay can't be used on the server, `null` when it can (as the engine decides). */
  function relayUnavailable(serverId: number): RelayUnavailable | null {
    const status = relayOf(serverId).status;
    switch (status.kind) {
      case "available":
        return null;
      case "none":
        return { kind: "noRelay" };
      case "unavailable":
        return status.reason;
      default:
        return { kind: status.kind };
    }
  }

  /** What blocks P2P features on the server, like the engine's plan (spec 04, FR-4.3 – FR-4.5). */
  function shareStartBlock(serverId: number): ShareStartBlock | null {
    const mode = requireServer(serverId).p2pModeOverride ?? settings.p2pMode;
    const relayReason = relayUnavailable(serverId);
    const direct = mode !== "relayOnly" && settings.allowDirectConnections !== "no";
    const relay = mode !== "directOnly" && relayReason === null;
    if (mode === "off") return { blocked: "p2pOff", relayReason: null };
    if (mode === "relayOnly" && relayReason !== null) {
      return { blocked: "relayUnavailable", relayReason };
    }
    return direct || relay ? null : { blocked: "consentOff", relayReason: null };
  }

  function setRelay(serverId: number, info: RelayInfo): void {
    relays.set(serverId, info);
    emit("relayChanged", { serverId, info });
  }

  function emit<K extends EventName>(event: K, payload: EventPayloads[K]): void {
    dispatch(event, payload);
    if (
      event === "serverSynced" ||
      event === "userUpserted" ||
      event === "userRemoved" ||
      event === "connectionStateChanged"
    ) {
      refreshFriends();
    }
  }

  /** Emits an event of the server the running command or control addresses. */
  function emitServer<K extends ServerEventName>(
    event: K,
    payload: Omit<EventPayloads[K], "serverId">,
  ): void {
    // The spread restores exactly the omitted key, which TS cannot see through the generic.
    emit(event, { ...payload, serverId: currentServerId } as EventPayloads[K]);
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

  function session(): MockSession | undefined {
    return sessions.get(currentServerId);
  }

  /** Sets the connection state of the current server; a disconnect ends its session. */
  function setState(next: ConnectionState): void {
    if (next.state === "disconnected") {
      sessions.delete(currentServerId);
      pmRequestsByServer.delete(currentServerId);
      if (pendingConnect?.serverId === currentServerId) pendingConnect = null;
      if (voiceServerId === currentServerId) setVoiceServer(null);
    } else {
      sessions.set(currentServerId, { snapshot: session()?.snapshot ?? null, state: next });
    }
    emitServer("connectionStateChanged", { state: next });
    syncActiveAudio();
  }

  function pmRequestsOf(serverId: number): PmRequests {
    return pmRequestsByServer.get(serverId) ?? { incoming: [], sent: [] };
  }

  function setPmRequests(serverId: number, requests: PmRequests): void {
    pmRequestsByServer.set(serverId, requests);
    emit("pmRequestsChanged", { serverId, requests });
  }

  /** The mock connects every server with the default identity. */
  function ownCertHash(): string {
    return identities.find((identity) => identity.isDefault)?.certHash ?? "";
  }

  function acceptConversation(name: string, certHash: string | null): void {
    const own = ownCertHash();
    if (certHash === null) return;
    if (acceptedConversations.some((c) => c.ownCertHash === own && c.certHash === certHash)) return;
    acceptedConversations = [
      ...acceptedConversations,
      { ownCertHash: own, certHash, name, acceptedAt: new Date().toISOString() },
    ];
    emit("acceptedConversationsChanged", { conversations: acceptedConversations });
  }

  /** Whether a block of the mock's identity covers `user` on `serverId`, like the engine. */
  function isBlocked(serverId: number, user: Pick<User, "name" | "certHash">): boolean {
    const own = ownCertHash();
    return blockedUsers.some(
      (b) =>
        b.ownCertHash === own &&
        (b.certHash === null
          ? b.serverId === serverId && b.name.toLowerCase() === user.name.toLowerCase()
          : b.certHash === user.certHash),
    );
  }

  function addBlock(serverId: number, name: string, certHash: string | null): void {
    const own = ownCertHash();
    const same = (b: BlockedUser) =>
      b.ownCertHash === own &&
      (certHash === null
        ? b.certHash === null &&
          b.serverId === serverId &&
          b.name.toLowerCase() === name.toLowerCase()
        : b.certHash === certHash);
    if (!blockedUsers.some(same)) {
      const serverLabel = servers.find((s) => s.id === serverId)?.label ?? "";
      blockedUsers = [
        ...blockedUsers,
        {
          id: nextBlockId++,
          ownCertHash: own,
          certHash,
          name,
          serverId,
          serverLabel,
          blockedAt: new Date().toISOString(),
        },
      ].sort((a, b) => a.name.localeCompare(b.name));
    }
    for (const [id, requests] of pmRequestsByServer) {
      const incoming = requests.incoming.filter((r) => !isBlocked(id, r));
      if (incoming.length !== requests.incoming.length)
        setPmRequests(id, { ...requests, incoming });
    }
    emit("blockedUsersChanged", { users: blockedUsers });
  }

  function setSnapshot(snapshot: ServerSnapshot): void {
    const current = sessions.get(snapshot.serverId);
    if (current) current.snapshot = snapshot;
  }

  function setVoiceServer(serverId: number | null): void {
    if (voiceServerId === serverId) return;
    voiceServerId = serverId;
    emit("voiceServerChanged", { serverId });
    syncActiveAudio();
  }

  /**
   * Like the engine: the profile of the own channel is in effect on the server with the voice
   * pipeline, the voice server or else the earliest connected one.
   */
  function syncActiveAudio(): void {
    const serverId =
      voiceServerId ??
      [...sessions].find(([, s]) => s.state.state === "connected" && s.snapshot)?.[0] ??
      null;
    const snap = serverId === null ? null : sessions.get(serverId)?.snapshot;
    const self = snap?.users.find((u) => u.session === snap.selfSession);
    const profile =
      serverId !== null && self ? channelAudio.get(serverId)?.get(self.channelId) : undefined;
    const next =
      serverId !== null && self && profile
        ? { serverId, channelId: self.channelId, profile }
        : null;
    if (JSON.stringify(next) === JSON.stringify(activeAudio)) return;
    activeAudio = next;
    emit("activeAudioProfileChanged", { active: next });
  }

  function channelAudioEntries(serverId: number) {
    return [...(channelAudio.get(serverId) ?? new Map<number, ChannelAudioProfile>())]
      .sort(([a], [b]) => a - b)
      .map(([channelId, profile]) => ({ channelId, profile }));
  }

  /**
   * Like the engine (FR-2.2): voice moves to the current server; the previous voice server
   * stays connected but becomes read-only (muted + deafened) again.
   */
  function takeVoice(): void {
    const target = currentServerId;
    const previous = voiceServerId;
    if (previous !== null && previous !== target && sessions.get(previous)?.snapshot) {
      currentServerId = previous;
      setSnapshot({ ...requireSnapshot(), readOnly: true, rejoinChannelId: null });
      updateUser(SELF_SESSION, { selfMute: true, selfDeaf: true });
      currentServerId = target;
    }
    setSnapshot({ ...requireSnapshot(), readOnly: false, rejoinChannelId: null });
    setVoiceServer(target);
  }

  /** Addresses `serverId`'s session; fails like the engine if it is not connected. */
  function requireSession(serverId: number): ServerSnapshot {
    currentServerId = serverId;
    return requireSnapshot();
  }

  /** Addresses the server the mock controls act on. */
  function focused(): void {
    currentServerId = focusedServerId;
  }

  function requireServer(id: number): SavedServer {
    return servers.find((s) => s.id === id) ?? fail("notFound", `no saved server ${String(id)}`);
  }

  function requireSnapshot(): ServerSnapshot {
    return session()?.snapshot ?? fail("notConnected", "not connected");
  }

  /** Deterministic 40-character certificate hash, like the engine's SHA-1. */
  function mockCertHash(seed: string): string {
    let hash = "";
    for (let index = 0; hash.length < 40; index += 1) {
      hash += (seed + String(index))
        .split("")
        .reduce((sum, char) => sum + char.charCodeAt(0), 0)
        .toString(16)
        .padStart(2, "0");
    }
    return hash.slice(0, 40);
  }

  function orderedIdentities(): Identity[] {
    return [...identities].sort(
      (a, b) =>
        Number(b.isDefault) - Number(a.isDefault) ||
        a.addedAt.localeCompare(b.addedAt) ||
        a.id - b.id,
    );
  }

  function defaultIdentity(): Identity | null {
    return identities.find((i) => i.isDefault) ?? null;
  }

  function requireIdentity(id?: number): Identity {
    if (id === undefined) return defaultIdentity() ?? fail("notFound", "no identity yet");
    return identities.find((i) => i.id === id) ?? fail("notFound", `no identity ${String(id)}`);
  }

  function addIdentity(label: string, name: string, certHash: string): Identity {
    if (certHash !== "" && identities.some((i) => i.certHash === certHash))
      fail("duplicateIdentity", "this identity is already in the list");
    const identity: Identity = {
      id: nextIdentityId++,
      label,
      name,
      certHash,
      addedAt: new Date().toISOString(),
      isDefault: identities.length === 0,
      serverIds: [],
    };
    identities = [...identities, identity];
    return identity;
  }

  /** Removes an identity from the mock; the default passes to the oldest remaining one. */
  function removeIdentity(id: number): void {
    const removed = requireIdentity(id);
    if (identities.length <= 1) fail("invalidInput", "Mumcord needs at least one identity");
    identities = identities.filter((i) => i.id !== id);
    if (removed.isDefault) {
      const next = orderedIdentities()[0];
      if (next) identities = identities.map((i) => ({ ...i, isDefault: i.id === next.id }));
    }
    for (const server of servers) {
      if (server.identityId === id) setIdentityOfServer(server.id, null);
    }
  }

  /** Stores a server's identity and keeps the identities' `serverIds` in sync. */
  function setIdentityOfServer(serverId: number, identityId: number | null): SavedServer {
    const server = requireServer(serverId);
    if (identityId !== null) requireIdentity(identityId);
    const updated: SavedServer = { ...server, identityId };
    servers = servers.map((s) => (s.id === serverId ? updated : s));
    identities = identities.map((i) => ({
      ...i,
      serverIds:
        i.id === identityId
          ? [...new Set([...i.serverIds, serverId])]
          : i.serverIds.filter((s) => s !== serverId),
    }));
    return updated;
  }

  function requireChannel(id: number): ChannelInfo {
    return (
      requireSnapshot().channels.find((c) => c.id === id) ??
      fail("notFound", `no channel ${String(id)}`)
    );
  }

  const watchParties = new Map<number, WatchParty>();
  let nextPlaylistEntry = 1;

  function changeParty(serverId: number, party: WatchParty): void {
    watchParties.set(serverId, party);
    emit("watchPartyChanged", { serverId, party });
  }
  // Only the user's own share; other people's shares come from `mock.emit`.
  const ownShares = new Map<number, ScreenShare>();

  function changeOwnShare(serverId: number, share: ScreenShare | null): void {
    if (share === null) ownShares.delete(serverId);
    else ownShares.set(serverId, share);
    const shares = share === null ? [] : [share];
    if (mediaDemo.share !== null && serverId === 1 && share?.session !== mediaDemo.share.session) {
      shares.push(mediaDemo.share);
    }
    emit("screenSharesChanged", { serverId, shares });
  }

  function requireOwnShare(serverId: number): ScreenShare {
    return ownShares.get(serverId) ?? fail("notFound", "no screen share");
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
    emitServer("userUpserted", { user: updated });
    if (session === SELF_SESSION) syncActiveAudio();
    return updated;
  }

  function selfUser(): User {
    return requireUser(SELF_SESSION);
  }

  /** Pretend size of the preview cache, freed by `previewCacheClear`. */
  let cachedPreviewBytes = 1_468_006;
  /** Pretend sizes of the other caches, freed by `cacheClear`. */
  let cachedWebKib = 188_416;
  let cachedTempKib = 352;
  // GIF favorites, most recently used first.
  let gifFavorites: string[] = [];
  /** Originals kept by `fullImageKeep`, by reference. */
  const fullImages = new Map<string, string>();
  const offeredHistory = new Map<
    number,
    { holderName: string; lines: readonly { senderName: string; body: string; sentAt: string }[] }
  >();

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

  /** Stores a synced message at its time position, as the engine's store orders by time. */
  function storeByTime(message: ChatMessage): void {
    const snap = requireSnapshot();
    if (!requireServer(snap.serverId).saveHistory) return;
    const map = serverHistory(snap.serverId);
    const key = conversationKey(message.conversation);
    const list = [...(map.get(key) ?? [])];
    const time = Date.parse(message.sentAt);
    let at = list.length;
    while (at > 0 && Date.parse(list[at - 1]?.sentAt ?? "") > time) at -= 1;
    list.splice(at, 0, message);
    map.set(key, list);
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
    const remember = !requireSnapshot().readOnly;
    updateUser(SELF_SESSION, change);
    const self = selfUser();
    // Undeafening a read-only server turns voice on there (FR-1.14).
    if (!self.selfDeaf && !remember) takeVoice();
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
        ? demoChannels(requireServer(serverId).label)
        : seedChannels();
    const target =
      (channelPath && findChannelByPath(channels, channelPath)) ??
      (stress ? findChannelByPath(channels, STRESS_SELF_CHANNEL_PATH) : undefined) ??
      channels[1];
    // Like the engine: a fresh connect lands read-only (muted + deafened), a link path joins.
    const readOnly = channelPath === null;
    // Like the engine: offered once, on the first fresh connect, if the channel still exists.
    const offerPath = rejoinOffers.get(serverId);
    rejoinOffers.delete(serverId);
    const offer = readOnly && offerPath ? findChannelByPath(channels, offerPath) : undefined;
    const self = {
      ...person(
        SELF_SESSION,
        requireServer(serverId).username,
        target?.id ?? 0,
        mockCertHash("zoe"),
      ),
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
    const users = stress
      ? stressUsers(self, channels)
      : demo
        ? demoUsers(self)
        : [self, anna, person(3, "Ben", 1, null), carla];
    const saved = requireServer(serverId);
    const info: ServerInfo = {
      version: serverVersion,
      followSupported: !serverVersion.startsWith("1.3") && !serverVersion.startsWith("1.2"),
      welcomeText: demo
        ? `Welcome to <b>${saved.label}</b>. Be kind and have fun!`
        : "Welcome to the <b>mock</b> server.",
      defaultChannelId: target?.id ?? null,
      maxMessageLength: 5000,
      maxImageMessageLength: 131072,
      allowHtml: true,
      release: null,
      os: "Linux",
      maxBandwidth: 128_000,
      maxUsers: 100,
      recordingAllowed: false,
      serverLabel: saved.label,
      serverAddress: saved.address,
      serverPort: saved.port,
      tlsFingerprint:
        "9F:2C:41:7A:B3:0E:D8:65:1C:AA:4B:F0:92:3D:77:E1:5A:08:C6:2F:B9:14:6E:D3:80:47:A5:1B:F2:69:CE:30",
      usersOnline: users.length,
      sessionId: SELF_SESSION,
      registered: true,
      currentChannelPermissions: 0x1 | 0x8 | 0x100,
      mumcordUsers: users.filter((user) => user.isMumcord).length,
      tlsTrusted: true,
      tlsIssuer: "CN=Example Root CA, O=Example Org",
      tlsValidFrom: "2026-03-01",
      tlsValidUntil: "2027-03-01",
      tlsCertificatePem:
        "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUExampleCertificateForTheMockServer\n-----END CERTIFICATE-----\n",
    };
    return {
      serverId,
      selfSession: SELF_SESSION,
      info,
      channels,
      users,
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
      withAvatar({ ...person(2, "Maya", 1, "mayahash"), hasComment: true, isMumcord: true }),
      withAvatar({ ...person(5, "Jonas", 1, "jonashash"), isMumcord: true }),
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
    currentServerId = serverId;
    focusedServerId = serverId;
    const snapshot = buildSnapshot(serverId, channelPath);
    sessions.set(serverId, { snapshot, state: { state: "connected", serverId } });
    if (!snapshot.readOnly) takeVoice();
    // The real engine reports the channels received while syncing (FR-2.24).
    emitServer("syncProgress", { channels: snapshot.channels.length });
    setState({ state: "connected", serverId });
    emitServer("transportChanged", { transport: "udp" });
    emit("serverSynced", { snapshot: requireSnapshot() });
    // The demo seeds show the media views with the mock's own fictional data (issue #163).
    if (mediaDemo.share !== null) emitServer("screenSharesChanged", { shares: [mediaDemo.share] });
    if (mediaDemo.party !== null) emitServer("watchPartyChanged", { party: mediaDemo.party });
  }

  function proceedConnect(
    serverId: number,
    channelPath: string[] | null,
    password: string | null,
  ): void {
    currentServerId = serverId;
    const required = requiredPasswords.get(serverId);
    if (required !== undefined && (password ?? savedPasswords.get(serverId)) !== required) {
      setState({
        state: "disconnected",
        error: { code: "wrongPassword", message: "wrong password" },
      });
      return;
    }
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
  ): Pick<SavedServer, "label" | "address" | "port" | "username"> {
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
    emitServer("unreadChanged", { conversation, count });
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
      syncedFrom: null,
    };
    store(message);
    return message;
  }

  // Import from the official Mumble client (issue #66).
  const mumble = options.mumble !== undefined ? options.mumble : demo ? DEMO_MUMBLE : null;
  let lastMumbleImport: { servers: number[]; identity: number | null } | null = null;

  function mumbleScan(): MumbleImportFound | null {
    if (mumble === null) return null;
    const known = new Set(servers.map((s) => `${s.address.toLowerCase()}:${String(s.port)}`));
    return {
      ...mumble,
      favourites: mumble.favourites.map((f) => ({
        ...f,
        duplicate: known.has(`${f.host.toLowerCase()}:${String(f.port)}`),
      })),
    };
  }

  function mumbleApply(
    request: Parameters<EngineCommands["mumbleImportApply"]>[0],
  ): MumbleImportDone {
    const found = mumbleScan();
    if (found === null) fail("notFound", "Mumble data not found");
    const cert = found.certificate;
    if (request.certificate && cert?.needsPassword === true) {
      if (request.certificatePassword !== DEMO_MUMBLE_CERT_PASSWORD)
        fail("wrongPassword", "the certificate could not be opened");
    }
    const done: MumbleImportDone = {
      added: [],
      skipped: [],
      passwordsSaved: 0,
      identity: null,
      identitySkipped: false,
      identityValidUntil: null,
    };
    if (request.certificate && cert) {
      if (cert.alreadyImported) done.identitySkipped = true;
      else {
        done.identity = addIdentity("Zoe", "Zoe", mockCertHash("mumble-import"));
        done.identityValidUntil = "2029-03-14T00:00:00Z";
      }
    }
    for (const f of found.favourites) {
      if (!request.favourites.some((k) => k.host === f.host && k.port === f.port)) continue;
      if (f.duplicate) {
        done.skipped.push(f.label);
        continue;
      }
      const hasPassword = request.passwords && f.hasPassword;
      const server: SavedServer = {
        id: nextServerId++,
        label: f.label,
        address: f.host,
        port: f.port,
        username: f.username,
        hasPassword,
        saveHistory: true,
        ...NEW_SERVER_FLAGS,
      };
      if (hasPassword) done.passwordsSaved += 1;
      servers = [...servers, server];
      done.added.push({ ...server });
    }
    lastMumbleImport = {
      servers: done.added.map((s) => s.id),
      identity: done.identity?.id ?? null,
    };
    return done;
  }

  function mumbleUndo(): void {
    const last = lastMumbleImport;
    if (last === null) fail("notFound", "nothing to undo");
    lastMumbleImport = null;
    servers = servers.filter((s) => !last.servers.includes(s.id));
    if (last.identity !== null) identities = identities.filter((i) => i.id !== last.identity);
  }

  const commands: EngineCommands = {
    // identity
    identityGet: () => Promise.resolve(defaultIdentity()),
    identityList: () => Promise.resolve(orderedIdentities().map((i) => ({ ...i }))),
    identityCreate: (name, label) => {
      if (name.trim() === "") fail("invalidInput", "name is required");
      const id = nextIdentityId;
      return Promise.resolve(
        addIdentity(label ?? name.trim(), name.trim(), mockCertHash(`created-${String(id)}`)),
      );
    },
    identityImport: (path, _password, label) => {
      const file = path.split(/[\\/]/).pop() ?? "Imported";
      const name = file.replace(/\.p12$|\.pfx$/i, "");
      // The hash comes from the file, so importing the same file twice is a duplicate.
      return Promise.resolve(addIdentity(label ?? name, name, mockCertHash(path)));
    },
    identityRename: (id, label) => {
      requireIdentity(id);
      if (label.trim() === "") fail("invalidInput", "label is required");
      let renamed = requireIdentity(id);
      renamed = { ...renamed, label: label.trim() };
      identities = identities.map((i) => (i.id === id ? renamed : i));
      return Promise.resolve(renamed);
    },
    identityExport: (id, _path, password) => {
      requireIdentity(id);
      if (password === "") fail("invalidInput", "a password is required");
      return Promise.resolve(null);
    },
    identityDelete: (id) => {
      removeIdentity(id);
      return Promise.resolve(null);
    },
    identitySetDefault: (id) => {
      requireIdentity(id);
      identities = identities.map((i) => ({ ...i, isDefault: i.id === id }));
      return Promise.resolve(orderedIdentities().map((i) => ({ ...i })));
    },

    mumbleImportScan: () => Promise.resolve(mumbleScan()),
    mumbleImportApply: (request) => Promise.resolve(mumbleApply(request)),
    mumbleImportUndo: () => {
      mumbleUndo();
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
        ...NEW_SERVER_FLAGS,
      };
      if (input.password.kind === "set") savedPasswords.set(server.id, input.password.value);
      servers = [...servers, server];
      return Promise.resolve({ ...server });
    },
    serverUpdate: (id, input) => {
      const existing = requireServer(id);
      const hasPassword =
        input.password.kind === "keep" ? existing.hasPassword : input.password.kind === "set";
      const updated: SavedServer = { ...existing, ...serverFields(input), hasPassword };
      if (input.password.kind === "set") savedPasswords.set(id, input.password.value);
      else if (input.password.kind === "clear") savedPasswords.delete(id);
      servers = servers.map((s) => (s.id === id ? updated : s));
      return Promise.resolve({ ...updated });
    },
    serverRemove: (id) => {
      requireServer(id);
      servers = servers.filter((s) => s.id !== id);
      identities = identities.map((i) => ({
        ...i,
        serverIds: i.serverIds.filter((s) => s !== id),
      }));
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
    // The mock never contacts a server. It answers with figures of its own, and says that nothing
    // was asked while the privacy switch is off — which is what the real command does (issue #175).
    mumbleLinkStats: (url: string, refresh: boolean): Promise<MumbleLinkStats> => {
      parseLink(url);
      if (!settings.linkPreviews) return Promise.resolve({ kind: "privacyOff" });
      // Like the engine, the mock keeps the answer for the session unless the card asks again.
      const stored = queryAnswers.get(url);
      const answer: MumbleLinkStats =
        refresh || stored === undefined
          ? { kind: "answered", pingMs: 12, users: 8, maxUsers: 24, version: serverVersion }
          : stored;
      queryAnswers.set(url, answer);
      return Promise.resolve(answer);
    },
    // The public server browser (issue #68): fictional servers, nothing is contacted.
    publicServersList: () => Promise.resolve(mockPublicServers()),
    publicServerPing: (host: string, port: number) =>
      Promise.resolve(mockPublicServerPing(host, port)),
    mumbleLinkJoin: (url: string) => {
      const link = parseLink(url);
      const saved = servers.find((s) => s.address === link.address && s.port === link.port);
      const outcome: LinkOutcome = saved
        ? { kind: "connecting", serverId: saved.id }
        : { kind: "unknownServer" };
      emit("deepLinkOpened", { link, outcome });
      return Promise.resolve(outcome);
    },

    // session
    connect: (serverId, channelPath, password) => {
      requireServer(serverId);
      requireIdentity();
      currentServerId = serverId;
      sessions.delete(serverId);
      if (voiceServerId === serverId) setVoiceServer(null);
      setState({ state: "connecting", serverId });
      later(() => {
        proceedConnect(serverId, channelPath, password);
      });
      return Promise.resolve(null);
    },
    disconnect: (serverId) => {
      if (!sessions.has(serverId)) return Promise.resolve(null);
      currentServerId = serverId;
      setState({ state: "disconnected", error: null });
      return Promise.resolve(null);
    },
    reconnectCancel: (serverId) => {
      if (sessions.get(serverId)?.state.state === "reconnecting") {
        currentServerId = serverId;
        setState({ state: "disconnected", error: null });
      }
      return Promise.resolve(null);
    },
    connectionsList: () =>
      Promise.resolve([...sessions].map(([serverId, s]) => ({ serverId, state: s.state }))),
    voiceServerGet: () => Promise.resolve(voiceServerId),
    sessionSnapshot: (serverId) => Promise.resolve(sessions.get(serverId)?.snapshot ?? null),
    connectionStats: (serverId) =>
      Promise.resolve(
        sessions.get(serverId)?.state.state === "connected"
          ? {
              ...connectionStats,
              // Follows the setting like the engine, unless a test set its own figure.
              latency:
                connectionStats.latency === DEFAULT_CONNECTION_STATS.latency
                  ? mockLatencyEstimate(activeAudio?.profile.lowLatency ?? settings.lowLatency)
                  : connectionStats.latency,
            }
          : null,
      ),
    certTrustDecide: (serverId, _fingerprint, trust) => {
      const pending = pendingConnect;
      if (pending?.serverId !== serverId) fail("invalidInput", "no trust decision pending");
      pendingConnect = null;
      currentServerId = serverId;
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
      currentServerId = voiceServerId ?? focusedServerId;
      const self = selfUser();
      changeSelfVoice(
        muted || !self.selfDeaf ? { selfMute: muted } : { selfMute: false, selfDeaf: false },
      );
      return Promise.resolve(null);
    },
    setSelfDeaf: (deafened) => {
      currentServerId = voiceServerId ?? focusedServerId;
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
    voiceLeave: () => {
      const previous = voiceServerId;
      if (previous === null) return Promise.resolve(null);
      // Like the engine: the server stays connected, read-only (muted + deafened).
      if (sessions.get(previous)?.snapshot) {
        currentServerId = previous;
        setSnapshot({ ...requireSnapshot(), readOnly: true, rejoinChannelId: null });
        updateUser(SELF_SESSION, { selfMute: true, selfDeaf: true });
      }
      setVoiceServer(null);
      return Promise.resolve(null);
    },

    channelJoin: (serverId, channelId) => {
      requireSession(serverId);
      // Like a real server, a refused join is answered asynchronously, here with the
      // channel-password prompt (FR-2.78).
      if (!requireChannel(channelId).permissions.enter) {
        emitServer("channelPasswordPrompt", { channelId, step: "required" });
        return Promise.resolve(null);
      }
      // An explicit join turns voice on with the last own mute/deafen choice.
      takeVoice();
      updateUser(SELF_SESSION, { channelId, ...joinedVoice() });
      return Promise.resolve(null);
    },

    channelJoinWithPassword: (serverId, channelId, password) => {
      requireSession(serverId);
      const channel = requireChannel(channelId);
      if (password === "") fail("invalidInput", "password must not be empty");
      if (password !== MOCK_CHANNEL_PASSWORD) {
        emitServer("channelPasswordPrompt", { channelId, step: "wrong" });
        return Promise.resolve(null);
      }
      const tokens = channelTokens.get(serverId) ?? [];
      const saved = { channelId, path: mockChannelPath(requireSnapshot().channels, channelId) };
      const known = tokens.find((t) => t.token === password);
      const lastUsedAt = new Date().toISOString();
      const token = known ?? { token: password, label: null, channels: [], lastUsedAt };
      token.channels = [...token.channels.filter((c) => c.channelId !== channelId), saved];
      token.lastUsedAt = lastUsedAt;
      if (!known) channelTokens.set(serverId, [...tokens, token]);
      const unlocked = { ...channel, permissions: { ...channel.permissions, enter: true } };
      const snap = requireSnapshot();
      snap.channels = snap.channels.map((c) => (c.id === channelId ? unlocked : c));
      emitServer("channelUpserted", { channel: unlocked });
      takeVoice();
      updateUser(SELF_SESSION, { channelId, ...joinedVoice() });
      emitServer("channelPasswordPrompt", { channelId, step: "ended" });
      return Promise.resolve(null);
    },

    channelPasswords: (serverId) => {
      const channels = sessions.get(serverId)?.snapshot?.channels;
      return Promise.resolve(
        (channelTokens.get(serverId) ?? []).map((t) => ({
          label: t.label,
          lastUsedAt: t.lastUsedAt,
          channels: t.channels.map((c) => ({
            ...c,
            found:
              channels === undefined
                ? null
                : mockChannelPath(channels, c.channelId).join("/") === c.path.join("/"),
          })),
        })),
      );
    },

    channelPasswordRename: (serverId, index, label) => {
      const token = channelTokens.get(serverId)?.[index];
      if (token === undefined) fail("invalidInput", "no channel password at this index");
      token.label = label.trim() === "" ? null : label.trim();
      return Promise.resolve(null);
    },

    channelPasswordsRemove: (serverId, indices) => {
      const tokens = channelTokens.get(serverId) ?? [];
      channelTokens.set(
        serverId,
        tokens.filter((_, index) => !indices.includes(index)),
      );
      return Promise.resolve(null);
    },

    channelDescriptionRequest: (serverId, channelId) => {
      requireSession(serverId);
      const channel = requireChannel(channelId);
      const text = MOCK_FETCHED_DESCRIPTIONS.get(channelId);
      if (channel.description === null && channel.hasDescription && text !== undefined) {
        const updated = { ...channel, description: text };
        const snap = requireSnapshot();
        snap.channels = snap.channels.map((c) => (c.id === channelId ? updated : c));
        emitServer("channelUpserted", { channel: updated });
      }
      return Promise.resolve(null);
    },

    // people
    personSetVolume: (serverId, session, volume) => {
      requireSession(serverId);
      if (!Number.isInteger(volume) || volume < 0 || volume > 200) {
        fail("invalidInput", "volume must be between 0 and 200");
      }
      updateUser(session, { volume });
      return Promise.resolve(null);
    },
    personMove: (serverId, session, channelId) => {
      requireSession(serverId);
      requireUser(session);
      // Like a real server, a refused move only answers with a denial; nobody moves.
      if (!requireChannel(channelId).permissions.moveUsers) return Promise.resolve(null);
      updateUser(session, { channelId });
      return Promise.resolve(null);
    },
    personSetLocalMute: (serverId, session, muted) => {
      requireSession(serverId);
      updateUser(session, { localMute: muted });
      return Promise.resolve(null);
    },
    personSetNote: (serverId, session, note) => {
      requireSession(serverId);
      requireUser(session);
      if (Array.from(note).length > 1000) fail("invalidInput", "note is too long");
      notes.set(session, note);
      return Promise.resolve(null);
    },
    friendsList: () => Promise.resolve(friendViews()),
    friendAdd: (_serverId, session) => {
      focused();
      if (session === SELF_SESSION) fail("invalidInput", "you cannot be your own friend");
      const user = requireUser(session);
      if (user.certHash === null) {
        fail("invalidInput", "people without a certificate cannot be friends");
      }
      const known = friends.get(user.certHash);
      friends.set(user.certHash, {
        name: user.name,
        addedAt: known?.addedAt ?? new Date().toISOString(),
        lastSeenAt: known?.lastSeenAt ?? null,
      });
      refreshFriends();
      return Promise.resolve(null);
    },
    pmRequests: (serverId) => Promise.resolve(pmRequestsOf(serverId)),
    pmRequestAccept: (serverId, id) => {
      requireSession(serverId);
      const requests = pmRequestsOf(serverId);
      const request = requests.incoming.find((r) => r.id === id);
      if (!request) fail("notFound", "no such request");
      acceptConversation(request.name, request.certHash);
      setPmRequests(serverId, {
        ...requests,
        incoming: requests.incoming.filter((r) => r.id !== id),
      });
      for (const message of request.messages) {
        store(message);
        emitServer("messageReceived", { message });
      }
      return Promise.resolve(null);
    },
    pmRequestDecline: (serverId, id) => {
      requireSession(serverId);
      const requests = pmRequestsOf(serverId);
      if (!requests.incoming.some((r) => r.id === id)) fail("notFound", "no such request");
      setPmRequests(serverId, {
        ...requests,
        incoming: requests.incoming.filter((r) => r.id !== id),
      });
      return Promise.resolve(null);
    },
    chatRequestSend: (serverId, session, note) => {
      requireSession(serverId);
      const user = requireUser(session);
      if (!user.isMumcord || !user.mumcordFeatures.includes("chat-request")) {
        fail("invalidInput", "this person cannot receive chat requests");
      }
      const requests = pmRequestsOf(serverId);
      if (requests.sent.some((r) => r.session === session && !r.accepted)) {
        fail("invalidInput", "a chat request is already pending");
      }
      if (note !== null && Array.from(note).length > 200) fail("invalidInput", "note is too long");
      acceptConversation(user.name, user.certHash);
      setPmRequests(serverId, {
        ...requests,
        sent: [
          ...requests.sent.filter((r) => r.session !== session),
          { session, name: user.name, certHash: user.certHash, accepted: false },
        ],
      });
      return Promise.resolve(null);
    },
    privateConversations: (serverId) => {
      requireSession(serverId);
      const list: PrivateConversation[] = [];
      for (const messages of serverHistory(serverId).values()) {
        const last = messages.at(-1);
        if (last?.conversation.kind !== "private") continue;
        const { certHash } = last.conversation;
        const peer = [...messages].reverse().find((m) => m.senderCertHash === certHash);
        list.push({ last, peerName: peer?.senderName ?? null });
      }
      list.sort((a, b) => Date.parse(b.last.sentAt) - Date.parse(a.last.sentAt));
      return Promise.resolve(list);
    },
    pmConversationPrefs: () => Promise.resolve(pmPrefs),
    pmConversationSetPinned: (serverId, certHash, pinned) => {
      setPmPrefs(serverId, certHash, { pinned });
      return Promise.resolve(null);
    },
    pmConversationMute: (serverId, certHash, minutes) => {
      setPmPrefs(
        serverId,
        certHash,
        minutes === null
          ? { mutedUntil: null, mutedForever: true }
          : {
              mutedUntil: new Date(Date.now() + minutes * 60_000).toISOString(),
              mutedForever: false,
            },
      );
      return Promise.resolve(null);
    },
    pmConversationUnmute: (serverId, certHash) => {
      setPmPrefs(serverId, certHash, { mutedUntil: null, mutedForever: false });
      return Promise.resolve(null);
    },
    blockedUsers: () => Promise.resolve(blockedUsers),
    blockUser: (serverId, session) => {
      requireSession(serverId);
      const user = requireUser(session);
      addBlock(serverId, user.name, user.certHash);
      return Promise.resolve(null);
    },
    blockCert: (serverId, certHash, name) => {
      if (certHash === "") return fail("invalidInput", "a certificate hash is needed");
      addBlock(serverId, name, certHash);
      return Promise.resolve(null);
    },
    unblock: (id) => {
      blockedUsers = blockedUsers.filter((b) => b.id !== id);
      emit("blockedUsersChanged", { users: blockedUsers });
      return Promise.resolve(null);
    },
    acceptedConversations: () => Promise.resolve(acceptedConversations),
    acceptedConversationRevoke: (ownCertHash, certHash) => {
      acceptedConversations = acceptedConversations.filter(
        (c) => c.ownCertHash !== ownCertHash || c.certHash !== certHash,
      );
      emit("acceptedConversationsChanged", { conversations: acceptedConversations });
      return Promise.resolve(null);
    },
    friendRemove: (certHash) => {
      friends.delete(certHash);
      refreshFriends();
      return Promise.resolve(null);
    },
    personProfile: (serverId, session) => {
      requireSession(serverId);
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
        note: notes.get(session) ?? "",
      });
    },

    // chat
    sendChannelMessage: (serverId, channelId, html) => {
      requireSession(serverId);
      if (!requireChannel(channelId).permissions.textMessage) {
        fail("permissionDenied", "You may not write in this channel.");
      }
      return Promise.resolve(sendMessage({ kind: "channel", channelId }, html));
    },
    sendPrivateMessage: (serverId, session, html) => {
      requireSession(serverId);
      const user = requireUser(session);
      const conversation: Conversation =
        user.certHash === null
          ? { kind: "privateSession", session }
          : { kind: "private", certHash: user.certHash };
      return Promise.resolve(sendMessage(conversation, html));
    },

    // follow
    channelFollow: (serverId, channelId) => {
      requireSession(serverId);
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
      emitServer("followedChannelsChanged", { channelIds: snap.followedChannelIds });
      return Promise.resolve(null);
    },
    channelUnfollow: (serverId, channelId) => {
      requireSession(serverId);
      const snap = requireSnapshot();
      snap.followedChannelIds = snap.followedChannelIds.filter((id) => id !== channelId);
      updateUser(SELF_SESSION, { listeningChannelIds: snap.followedChannelIds });
      emitServer("followedChannelsChanged", { channelIds: snap.followedChannelIds });
      return Promise.resolve(null);
    },
    channelFollowAll: (serverId) => {
      requireSession(serverId);
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
        emitServer("followedChannelsChanged", { channelIds: followed });
      }
      return Promise.resolve(summary);
    },
    channelUnfollowAll: (serverId) => {
      requireSession(serverId);
      const snap = requireSnapshot();
      snap.followedChannelIds = [];
      updateUser(SELF_SESSION, { listeningChannelIds: [] });
      emitServer("followedChannelsChanged", { channelIds: [] });
      return Promise.resolve(null);
    },
    conversationMarkRead: (serverId, conversation) => {
      requireSession(serverId);
      setUnread(conversation, 0);
      return Promise.resolve(null);
    },

    // history
    historyLoad: (serverId, conversation, before, limit) => {
      requireSession(serverId);
      const all = serverHistory(serverId).get(conversationKey(conversation)) ?? [];
      const end = before === null ? all.length : all.findIndex((m) => m.id === before);
      const stop = end === -1 ? all.length : end;
      return Promise.resolve(all.slice(Math.max(0, stop - limit), stop));
    },
    historyClear: (scope: HistoryScope) => {
      const now = new Date().toISOString();
      if (scope.kind === "all") {
        history.clear();
        clearedAt.set(clearedKey(null, null), now);
      } else if (scope.kind === "server") {
        history.delete(scope.serverId);
        clearedAt.set(clearedKey(scope.serverId, null), now);
      } else {
        serverHistory(scope.serverId).delete(conversationKey(scope.conversation));
        if (scope.conversation.kind === "channel") {
          clearedAt.set(clearedKey(scope.serverId, scope.conversation.channelId), now);
        }
      }
      return Promise.resolve(null);
    },
    historyClearedAt: (serverId, channelId) => {
      requireSession(serverId);
      const marks = [
        clearedAt.get(clearedKey(serverId, channelId)),
        clearedAt.get(clearedKey(serverId, null)),
        clearedAt.get(clearedKey(null, null)),
      ].filter((at) => at !== undefined);
      return Promise.resolve(marks.sort().at(-1) ?? null);
    },
    historySetSaving: (serverId, enabled) => {
      requireServer(serverId);
      servers = servers.map((s) => (s.id === serverId ? { ...s, saveHistory: enabled } : s));
      return Promise.resolve(null);
    },

    // profile
    setOwnAvatar: (serverId, path) => {
      requireSession(serverId);
      updateUser(SELF_SESSION, {
        hasAvatar: path !== null,
        avatarHash: path === null ? null : `own-${path}`,
      });
      return Promise.resolve(null);
    },
    setOwnComment: (serverId, html) => {
      requireSession(serverId);
      checkSize(html);
      updateUser(SELF_SESSION, { hasComment: html !== "" });
      return Promise.resolve(null);
    },
    defaultAvatarGet: () => Promise.resolve(defaultAvatar),
    defaultAvatarSet: (path) => {
      defaultAvatar = path === null ? null : MOCK_DEFAULT_AVATAR;
      return Promise.resolve(defaultAvatar);
    },
    registerSelf: (serverId) => {
      requireSession(serverId);
      const root = requireSnapshot().channels.find((c) => c.parentId === null);
      if (!root?.permissions.registerSelf) fail("permissionDenied", "Registration is not allowed.");
      updateUser(SELF_SESSION, { registered: true });
      return Promise.resolve(null);
    },

    // links
    openLink: (url) => {
      // Browser preview without Tauri: a new tab without access to this page.
      window.open(url, "_blank", "noopener,noreferrer");
      return Promise.resolve(null);
    },
    linkPreview: (url) =>
      Promise.resolve(settings.linkPreviews ? mockLinkPreview(url, demo) : null),
    inlineImage: (url) =>
      Promise.resolve(settings.linkPreviews ? mockInlineImage(url, demo) : null),
    audioInfo: (url) => Promise.resolve(settings.linkPreviews ? mockAudioInfo(url) : null),
    audioFile: (url) =>
      Promise.resolve(
        settings.linkPreviews && mockAudioInfo(url) !== null ? mockAudioFile() : null,
      ),
    audioSave: () => Promise.resolve(null),
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
    copyImage: () => Promise.resolve(null),
    saveText: () => Promise.resolve(null),
    // The browser's own clipboard stands in for the system one.
    copyText: (text) => navigator.clipboard.writeText(text).then(() => null),
    // The browser has no native file drops; dropped files arrive as `File` objects instead.
    readDroppedImage: () =>
      Promise.reject(new EngineError("notFound", "no native file drops in the mock")),
    // Browsers hand pasted files to the page themselves; there is no native clipboard to read.
    clipboardFiles: () => Promise.resolve([]),
    clipboardImage: () => Promise.resolve(null),
    // Full images (FR-1.60c): kept originals are "fetched" back at once, with one progress event.
    fullImageKeep: (dataUrl) => {
      if (!settings.offerFullImages)
        fail("invalidInput", "offering full-size images is switched off");
      let hash = 0x811c9dc5;
      for (let i = 0; i < dataUrl.length; i++)
        hash = Math.imul(hash ^ dataUrl.charCodeAt(i), 16777619);
      const hex = (hash >>> 0).toString(16).padStart(8, "0").repeat(8);
      const reference = `${hex}:${String(Math.max(1, Math.floor(dataUrl.length * 0.75)))}`;
      fullImages.set(reference, dataUrl);
      return Promise.resolve(reference);
    },
    historySyncRequest: (serverId, channelId) => {
      requireSession(serverId);
      const snap = requireSnapshot();
      if (!snap.info.followSupported)
        fail("serverTooOld", "history sync needs a Mumble 1.4+ server");
      if (channelId !== selfUser().channelId && !snap.followedChannelIds.includes(channelId))
        fail("notFound", "channel is neither joined nor followed");
      emitServer("historySyncStatus", { channelId, running: true, added: 0 });
      const offer = offeredHistory.get(channelId);
      offeredHistory.delete(channelId);
      const messages: ChatMessage[] = (offer?.lines ?? []).map((line) => ({
        id: String(nextMessageId++),
        conversation: { kind: "channel", channelId },
        senderName: line.senderName,
        senderCertHash: null,
        senderSession: null,
        own: false,
        sentAt: line.sentAt,
        body: line.body,
        dismissedPreviews: [],
        syncedFrom: offer?.holderName ?? null,
      }));
      messages.sort((a, b) => Date.parse(a.sentAt) - Date.parse(b.sentAt));
      for (const message of messages) storeByTime(message);
      if (messages.length > 0) emitServer("historySynced", { channelId, messages });
      emitServer("historySyncStatus", { channelId, running: false, added: messages.length });
      return Promise.resolve(null);
    },
    fullImageFetch: (serverId, reference) => {
      requireSession(serverId);
      const src = settings.loadFullImages ? (fullImages.get(reference) ?? null) : null;
      if (src !== null) emitServer("fullImageProgress", { reference, received: 1, total: 1 });
      return Promise.resolve(src);
    },
    previewCacheClear: () => {
      const freed = cachedPreviewBytes;
      cachedPreviewBytes = 0;
      return Promise.resolve(freed);
    },

    cacheSizes: () =>
      Promise.resolve({
        previewsKib: Math.ceil(cachedPreviewBytes / 1024),
        webKib: cachedWebKib,
        tempKib: cachedTempKib,
      }),
    cacheClear: (kinds) => {
      let freedKib = 0;
      if (kinds.includes("previews")) {
        freedKib += Math.ceil(cachedPreviewBytes / 1024);
        cachedPreviewBytes = 0;
      }
      if (kinds.includes("webEngine")) {
        freedKib += cachedWebKib;
        cachedWebKib = 0;
      }
      if (kinds.includes("temp")) {
        freedKib += cachedTempKib;
        cachedTempKib = 0;
      }
      return Promise.resolve({ freedKib, webPending: kinds.includes("webEngine") });
    },

    // now playing: no media player in the mock; other people's tracks come from `mock.emit`.
    nowPlayingStatus: () => Promise.resolve({ supported: true, track: null }),
    nowPlayingCover: () => Promise.resolve(null),

    // now gaming: no game detection in the mock; other people's games come from `mock.emit`.
    nowGamingStatus: () => Promise.resolve({ supported: true, game: null }),
    nowGamingIcon: () => Promise.resolve(null),
    nowGamingJoin: () => Promise.resolve(null),

    // positional audio: no game delivers positions in the mock; "3D" icons come from `mock.emit`.
    positionalStatus: () =>
      Promise.resolve({
        enabled: settings.positional.enabled,
        source: null,
        game: null,
        senders: [],
      }),

    gamePluginsList: () => Promise.resolve(gamePlugins.map((p) => ({ ...p }))),
    gamePluginSetEnabled: (id, enabled) => {
      if (!gamePlugins.some((p) => p.id === id)) fail("notFound", "no such game plugin");
      gamePlugins = gamePlugins.map((p) => (p.id === id ? { ...p, enabled } : p));
      return Promise.resolve(gamePlugins.map((p) => ({ ...p })));
    },

    // in-game overlay (FR-1.46b)
    ingameOverlayState: () => Promise.resolve(ingameOverlay),
    ingameOverlaySetMode: (mode) => {
      const games = ingameOverlay.games.map((g) => ({ ...g, effective: effectOf({ ...g }) }));
      return setIngameOverlay({ ...ingameOverlay, mode, games });
    },
    ingameOverlaySetGame: (entry) => {
      const withEffect = { ...entry, effective: effectOf(entry) };
      const games = ingameOverlay.games.filter((g) => !sameKey(g.key, entry.key));
      games.push(withEffect);
      const known = ingameOverlay.known.filter((k) => !sameKey(k.key, entry.key));
      return setIngameOverlay({ ...ingameOverlay, games, known });
    },
    ingameOverlayRemoveGame: (key) =>
      setIngameOverlay({
        ...ingameOverlay,
        games: ingameOverlay.games.filter((g) => !sameKey(g.key, key)),
      }),

    // settings
    settingsGet: () => Promise.resolve({ ...settings }),
    settingsDefault: () => Promise.resolve({ ...DEFAULT_SETTINGS }),
    settingsUpdate: (next) => {
      const positionalSwitched = next.positional.enabled !== settings.positional.enabled;
      settings = { ...next };
      if (positionalSwitched) {
        emit("positionalStatusChanged", {
          status: { enabled: settings.positional.enabled, source: null, game: null, senders: [] },
        });
      }
      return Promise.resolve({ ...settings });
    },

    // audio
    audioDevices: () => Promise.resolve(DEVICES),
    bitrateMax: () => Promise.resolve<number | null>(null),
    latencyEstimate: () =>
      Promise.resolve(mockLatencyEstimate(activeAudio?.profile.lowLatency ?? settings.lowLatency)),
    channelAudioProfiles: (serverId) => {
      requireSession(serverId);
      return Promise.resolve(channelAudioEntries(serverId));
    },
    channelAudioProfileSet: (serverId, channelId, profile) => {
      requireSession(serverId);
      requireChannel(channelId);
      const kbps = profile?.bitrateKbps;
      if (kbps != null && (kbps < 16 || kbps > 510)) {
        fail("invalidInput", "bitrate must be 16–510 kbit/s");
      }
      const saved = channelAudio.get(serverId) ?? new Map<number, ChannelAudioProfile>();
      if (profile) saved.set(channelId, { ...profile });
      else saved.delete(channelId);
      channelAudio.set(serverId, saved);
      emitServer("channelAudioProfilesChanged", { profiles: channelAudioEntries(serverId) });
      syncActiveAudio();
      return Promise.resolve(null);
    },
    activeAudioProfile: () => Promise.resolve(activeAudio),
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
    audioTestSound: () => Promise.resolve(null),
    micTestStop: () => {
      clearInterval(micTimer);
      clearTimeout(micPhaseTimer);
      micTimer = undefined;
      return Promise.resolve(null);
    },

    // whisper and shout
    whisperSetup: () => Promise.resolve(whisperSetup()),
    whisperTargetsSave: (targets) => {
      const keys = targets.flatMap((target) => (target.hotkey === null ? [] : [target.hotkey]));
      if (new Set(keys).size !== keys.length || keys.includes(settings.pttKey ?? "")) {
        return Promise.reject(new EngineError("invalidInput", "whisper key already in use"));
      }
      whisperTargets = targets;
      return Promise.resolve(whisperSetup());
    },
    whisperKeyCheck: (targetId, key) => {
      const taken =
        key === settings.pttKey ||
        whisperTargets.some((target) => target.id !== targetId && target.hotkey === key);
      return taken
        ? Promise.reject(new EngineError("invalidInput", "whisper key already in use"))
        : Promise.resolve(key.replace(/^Key|(?<=\+)Key/g, ""));
    },
    whisperStart: (targetId) => {
      const state: WhisperState = whisperTargets.some((target) => target.id === targetId)
        ? { state: "active", targetId, held: false }
        : { state: "denied", targetId, reason: "unknownTarget", channel: null };
      emit("whisperStateChanged", { state });
      return Promise.resolve(state);
    },
    whisperStop: () => {
      emit("whisperStateChanged", { state: { state: "idle" } });
      return Promise.resolve(null);
    },

    // push-to-talk
    pttAvailability: () => Promise.resolve<PttAvailability>({ status: "available" }),
    // Mouse buttons come back unchanged (no desktop dialog), keyboard keys as "F13" (the demo
    // keeps the pressed key so screenshots show a realistic shortcut).
    pttCaptureKey: (key) => Promise.resolve(demo || /^Mouse(Middle|4|5)$/.test(key) ? key : "F13"),
    pttMouseStatus: () => Promise.resolve<PttMouseStatus>({ support: "native", state: "ready" }),
    pttCaptureCancel: () => Promise.resolve(null),

    // global hotkeys (#64)
    hotkeyCheck: (key, slot) => {
      const canonical = mockHotkey(key);
      const same = (other: string | null) => other !== null && mockHotkey(other) === canonical;
      const inApp = MOCK_IN_APP_SHORTCUTS[canonical];
      const index = settings.hotkeys.findIndex(
        (h, i) => !(slot.kind === "hotkey" && slot.index === i) && same(h.key),
      );
      const conflict: HotkeyConflict | null =
        inApp !== undefined
          ? { kind: "inApp", shortcut: inApp }
          : slot.kind !== "pushToTalk" && same(settings.pttKey)
            ? { kind: "pushToTalk" }
            : index >= 0
              ? { kind: "hotkey", index }
              : null;
      return Promise.resolve({ key: canonical, conflict });
    },
    hotkeysStatus: () => Promise.resolve<HotkeysStatus>({ mode: "global", failed: [] }),
    pttSession: () =>
      Promise.resolve({ waylandDesktop: false, compatibilityActive: false, kde: false }),
    pttOpenKdeX11Settings: () => Promise.resolve(null),
    pttKdeX11Access: () => Promise.resolve({ allowed: false, undoAvailable: false }),
    pttKdeX11AllowMouse: () => Promise.resolve({ allowed: true, undoAvailable: true }),
    pttKdeX11Undo: () => Promise.resolve({ allowed: false, undoAvailable: false }),

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
    pluginsTakePendingLink: () => Promise.resolve(null),

    // plugins (spec 06): no plugin is installed in the mock; UI tasks extend this.
    pluginsList: () => Promise.resolve({ noPlugins: false, allOff: false, plugins: [] }),
    pluginsIndex: () => Promise.resolve({ freshness: "fresh", notice: null, plugins: [] }),
    pluginInstall: () => notImplemented("pluginInstall"),
    pluginSuggestionNotNow: () => Promise.resolve(null),
    pluginSuggestionsDismissServer: () => Promise.resolve(null),
    pluginSetEnabled: () => fail("notFound", "no such plugin"),
    pluginSetPermissions: () => fail("notFound", "no such plugin"),
    pluginSetVisibility: () => fail("notFound", "no such plugin"),
    pluginSetNotifications: () => fail("notFound", "no such plugin"),
    pluginRemove: () => fail("notFound", "no such plugin"),
    pluginExportData: () => fail("notFound", "no such plugin"),
    pluginResetData: () => fail("notFound", "no such plugin"),
    pluginDevLoad: () => notImplemented("pluginDevLoad"),
    pluginDevReload: () => fail("notFound", "no such plugin"),
    pluginDevUnload: () => fail("notFound", "no such plugin"),
    pluginsSetAllOff: () => Promise.resolve(null),
    pluginSetBadge: () => Promise.resolve(null),
    pluginDiagnostics: () => fail("notFound", "no such plugin"),
    pluginsUiTrees: () => Promise.resolve([]),
    pluginsSetAppearance: () => Promise.resolve(null),
    pluginUiAction: () => fail("invalidInput", "no such action on screen"),
    pluginUiEvent: () => fail("invalidInput", "the plugin is not running"),
    pluginsMentioned: () => Promise.resolve(null),

    // packet inspector
    packetCaptureSet: (enabled) => {
      inspector.set(enabled);
      return Promise.resolve(null);
    },
    packetCaptureBackground: (enabled) => {
      inspector.setBackground(enabled);
      return Promise.resolve(null);
    },
    packetCapturePoll: (after) => Promise.resolve(inspector.poll(after)),
    packetCaptureClear: () => {
      inspector.clear();
      return Promise.resolve(null);
    },
    packetCaptureExport: () => Promise.resolve(null),
    // The mock has no second window; the inspector page opens with `?window=packet-inspector`.
    packetInspectorOpen: () => Promise.resolve(null),
    packetInspectorClose: () => Promise.resolve(null),
    packetInspectorShare: (view) => {
      inspectorView = view;
      emit("inspectorViewChanged", { view });
      return Promise.resolve(null);
    },
    packetInspectorView: () => Promise.resolve(inspectorView),
    licencesOpen: () => Promise.resolve(null),
    licencesClose: () => Promise.resolve(null),
    changelogOpen: () => Promise.resolve(null),
    changelogClose: () => Promise.resolve(null),
    // The mock has no second window; the pop-out page opens with `?window=media-popout&kind=…`.
    mediaPopoutOpen: (kind, target, _title, onTop) => {
      popouts.open(kind, target, onTop);
      return Promise.resolve(null);
    },
    mediaPopoutClose: (kind) => {
      popouts.close(kind);
      return Promise.resolve(null);
    },
    mediaPopoutSetOnTop: (kind, on) => {
      popouts.setOnTop(kind, on);
      return Promise.resolve(null);
    },
    mediaPopoutSetTitle: () => Promise.resolve(null),
    mediaPopoutState: () => Promise.resolve(popouts.state()),
    whatsNewMarkSeen: () => {
      settings = { ...settings, whatsNewSeen: "0.0.0-mock" };
      return Promise.resolve(null);
    },
    windowLookShare: (look) => {
      windowLook = look;
      emit("windowLookChanged", { look });
      return Promise.resolve(null);
    },
    windowLook: () => Promise.resolve(windowLook),

    // diagnostics
    diagnosticsSave: () => Promise.resolve(null),
    appInfo: () =>
      Promise.resolve(
        demo
          ? {
              version: "0.3.0",
              packageVersion: "0.3.0",
              os: "Linux x86_64",
              appimage: false,
              portable: false,
              distro: null,
            }
          : {
              version: "0.0.0-mock",
              packageVersion: "0.0.0-mock",
              os: "mock",
              appimage: false,
              portable: false,
              distro: null,
            },
      ),

    // message notifications: same decision as the engine (src-tauri/src/notifications.rs)
    notifyMessage: (notice) => {
      if (dndActive()) return Promise.resolve("none");
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
    trayAttentionSet: () => Promise.resolve(null),
    dndState: () => Promise.resolve(dnd),
    dndSet: (on) => setDnd({ ...dnd, manual: on }),
    dndSetFollowSystem: (on) => setDnd({ ...dnd, followSystem: on }),

    // Epic 2 contract, not built in the engine yet.
    serverSetAlwaysConnect: (serverId, enabled) => {
      requireServer(serverId);
      const marked = servers.filter((s) => s.alwaysConnectAtStart && s.id !== serverId).length;
      if (enabled && marked >= settings.maxConnectedServers) {
        fail(
          "serverLimitReached",
          `at most ${String(settings.maxConnectedServers)} servers can be connected at the same time`,
        );
      }
      servers = servers.map((s) =>
        s.id === serverId ? { ...s, alwaysConnectAtStart: enabled } : s,
      );
      return Promise.resolve({ ...requireServer(serverId) });
    },
    serverSetNotificationsMuted: (serverId, muted) => {
      requireServer(serverId);
      servers = servers.map((s) => (s.id === serverId ? { ...s, notificationsMuted: muted } : s));
      return Promise.resolve({ ...requireServer(serverId) });
    },
    serverSetGroupMentionsMuted: (serverId, muted) => {
      requireServer(serverId);
      servers = servers.map((s) => (s.id === serverId ? { ...s, groupMentionsMuted: muted } : s));
      return Promise.resolve({ ...requireServer(serverId) });
    },
    serverSetP2pModeOverride: (serverId, mode) => {
      requireServer(serverId);
      servers = servers.map((s) => (s.id === serverId ? { ...s, p2pModeOverride: mode } : s));
      return Promise.resolve({ ...requireServer(serverId) });
    },
    serverSetIdentity: (serverId, identityId) =>
      Promise.resolve({ ...setIdentityOfServer(serverId, identityId) }),
    serverTestConnection: (input) => {
      if (input.address.trim() === "") fail("invalidInput", "address is required");
      if (input.port === 0) fail("invalidInput", "port is required");
      if (input.username.trim() === "") fail("invalidInput", "username is required");
      const server = input.serverId === null ? null : requireServer(input.serverId);
      // The mock knows one reachable server; anything else is unreachable.
      if (!["localhost", "127.0.0.1"].includes(input.address.trim()))
        fail("serverUnreachable", "unreachable in the mock");
      if (input.password.kind === "set" && input.password.value === "wrong")
        fail("wrongPassword", "wrong password");
      const certificate =
        server === null ? "unknown" : certPrompts.has(server.id) ? "changed" : "trusted";
      return Promise.resolve({
        pingMs: 12,
        version: "1.5.0",
        fingerprint: MOCK_FINGERPRINT,
        certificate,
      });
    },
    serverSetTheme: (serverId, themeId) => {
      requireServer(serverId);
      const theme = themeId?.trim() ? themeId.trim() : null;
      servers = servers.map((s) => (s.id === serverId ? { ...s, themeId: theme } : s));
      return Promise.resolve({ ...requireServer(serverId) });
    },
    fullImageCancel: () => Promise.resolve(null),
    userStatsWatch: () => Promise.resolve(null),
    p2pConsentDecide: (allow) => {
      settings = { ...settings, allowDirectConnections: allow ? "yes" : "no" };
      return Promise.resolve(null);
    },
    // Demo only, for screenshots: Maya (session 2) is reached through the relay, Jonas
    // (session 5) directly. Priya (session 7), whose share the media views show, is reached
    // directly as well, so the path the share view names matches its own chip: media goes over a
    // direct connection today (FR-2.46).
    p2pLink: (_serverId, session) =>
      Promise.resolve(
        !demo
          ? null
          : session === 2
            ? { path: "relayTcp", ipHidden: true }
            : session === 5 || session === mediaDemo.share?.session
              ? { path: "direct", ipHidden: false }
              : null,
      ),
    relayInfo: (serverId) => Promise.resolve(relayOf(serverId)),
    relayTrustNew: (serverId) => {
      setRelay(serverId, { ...relayOf(serverId), status: { kind: "available" } });
      return Promise.resolve(null);
    },
    relayKeepBlocked: (serverId) => {
      setRelay(serverId, {
        ...relayOf(serverId),
        status: { kind: "untrusted", keptBlocked: true },
      });
      return Promise.resolve(null);
    },
    relayForget: (serverId) => {
      setRelay(serverId, NO_RELAY);
      return Promise.resolve(null);
    },
    // watch party: only the user's own party; other people's actions come from `mock.emit`.
    watchPartyStart: (serverId, videoId) => {
      const party: WatchParty = {
        channelId: requireUser(SELF_SESSION).channelId,
        videoId,
        playing: true,
        positionMs: 0,
        participants: [SELF_SESSION],
        lastAction: null,
        joined: true,
        currentEntry: 0,
        playlist: [],
        skippedBy: null,
        positions: [],
        host: SELF_SESSION,
        history: [],
      };
      watchParties.set(serverId, party);
      emit("watchPartyChanged", { serverId, party });
      return Promise.resolve(party);
    },
    watchPartyJoin: (serverId) => {
      const current = watchParties.get(serverId);
      if (!current) fail("notFound", "no watch party in this channel");
      const party: WatchParty = {
        ...current,
        joined: true,
        participants: [...new Set([...current.participants, SELF_SESSION])],
      };
      watchParties.set(serverId, party);
      emit("watchPartyChanged", { serverId, party });
      return Promise.resolve(party);
    },
    watchPartyCommand: (serverId, command) => {
      const current = watchParties.get(serverId);
      if (!current?.joined) fail("notFound", "no watch party in this channel");
      const party: WatchParty = {
        ...current,
        playing: command.kind === "seek" ? current.playing : command.kind === "play",
        positionMs: command.positionMs,
        lastAction: { session: SELF_SESSION, command },
        skippedBy: null,
      };
      watchParties.set(serverId, party);
      emit("watchPartyChanged", { serverId, party });
      return Promise.resolve(null);
    },
    watchPartyGet: (serverId) => Promise.resolve(watchParties.get(serverId) ?? mediaDemo.party),
    watchPartyAdd: (serverId, videoId) => {
      const current = watchParties.get(serverId);
      if (!current?.joined) fail("notFound", "no watch party in this channel");
      if (current.playlist.length >= 50) fail("invalidInput", "the playlist is full");
      const id = nextPlaylistEntry++;
      changeParty(serverId, {
        ...current,
        playlist: [
          ...current.playlist,
          { id, videoId, addedBy: SELF_SESSION, addedByName: requireUser(SELF_SESSION).name },
        ],
      });
      return Promise.resolve(id);
    },
    watchPartyRemove: (serverId, entry) => {
      const current = watchParties.get(serverId);
      if (!current?.joined) fail("notFound", "no watch party in this channel");
      changeParty(serverId, {
        ...current,
        playlist: current.playlist.filter((e) => e.id !== entry),
      });
      return Promise.resolve(null);
    },
    watchPartyMove: (serverId, entry, index) => {
      const current = watchParties.get(serverId);
      if (!current?.joined) fail("notFound", "no watch party in this channel");
      const moved = current.playlist.find((e) => e.id === entry);
      if (!moved) fail("notFound", "no such playlist entry");
      const playlist = current.playlist.filter((e) => e.id !== entry);
      playlist.splice(index, 0, moved);
      changeParty(serverId, { ...current, playlist });
      return Promise.resolve(null);
    },
    // The past the mock keeps for itself, the same ten entries the engine remembers (issue #163).
    watchPartyNext: (serverId, from, skip) => {
      const current = watchParties.get(serverId);
      if (!current?.joined) fail("notFound", "no watch party in this channel");
      const [next, ...rest] = current.playlist;
      if (current.currentEntry !== from || next === undefined) return Promise.resolve(null);
      changeParty(serverId, {
        ...current,
        videoId: next.videoId,
        currentEntry: next.id,
        playing: true,
        positionMs: 0,
        lastAction: null,
        skippedBy: skip ? SELF_SESSION : null,
        playlist: rest,
        history: [...current.history, { id: from, videoId: current.videoId }].slice(-10),
      });
      return Promise.resolve(null);
    },
    watchPartyPrevious: (serverId) => {
      const current = watchParties.get(serverId);
      if (!current?.joined) fail("notFound", "no watch party in this channel");
      if (current.history.length === 0) fail("invalidInput", "nothing has played yet");
      const history = [...current.history];
      const item = history.pop();
      if (item === undefined) fail("invalidInput", "nothing has played yet");
      changeParty(serverId, {
        ...current,
        videoId: item.videoId,
        currentEntry: item.id,
        playing: true,
        positionMs: 0,
        lastAction: null,
        skippedBy: null,
        history,
      });
      return Promise.resolve(null);
    },
    // No loopback server in the browser: the page URLs only let the player frames mount.
    watchPartyReport: () => Promise.resolve(null),
    screenShareQuality: (serverId) => {
      // The mock's fictional share carries the fictional figures, so the media views show numbers.
      return Promise.resolve(mediaDemo.share !== null && serverId === 1 ? mediaDemo.quality : null);
    },
    // The mock has no capture, so the bar shows its own placeholder picture.
    screenShareThumbnail: (serverId) => {
      if (!ownShares.has(serverId)) return Promise.resolve(null);
      return Promise.resolve({ width: 320, height: 180, dataUrl: DEMO_PHOTO });
    },
    screenShareWatchStats: (_serverId, session) => {
      if (mediaDemo.share !== null && mediaDemo.share.session === session) {
        return Promise.resolve({ path: "direct", rms: 0.42 });
      }
      return Promise.resolve(null);
    },
    watchPartyPlayerUrl: () =>
      Promise.resolve("http://localhost:43215/0123456789abcdef0123456789abcdef/player.html"),
    embedPlayerOpen: () =>
      Promise.resolve("http://localhost:43215/0123456789abcdef0123456789abcdef/embed.html"),
    embedPlayerClose: () => Promise.resolve(null),
    watchPartyLeave: (serverId) => {
      watchParties.delete(serverId);
      emit("watchPartyChanged", { serverId, party: null });
      return Promise.resolve(null);
    },
    // The browser has no system picker: starting shares at once, without video.
    screenShareStart: (serverId, withSound) => {
      if (ownShares.has(serverId)) fail("invalidInput", "already sharing on this server");
      changeOwnShare(serverId, {
        session: SELF_SESSION,
        paused: false,
        sound: withSound,
        soundAvailable: withSound,
        kind: null,
        elapsedMs: 0,
        viewers: [],
        blocked: null,
        relayReason: null,
      });
      return Promise.resolve(null);
    },
    screenShareStop: (serverId) => {
      requireOwnShare(serverId);
      changeOwnShare(serverId, null);
      return Promise.resolve(null);
    },
    screenShareSetPaused: (serverId, paused) => {
      changeOwnShare(serverId, { ...requireOwnShare(serverId), paused });
      return Promise.resolve(null);
    },
    screenShareSetSoundLevel: () => Promise.resolve(null),
    screenShareSetSound: (serverId, enabled) => {
      changeOwnShare(serverId, {
        ...requireOwnShare(serverId),
        sound: enabled && requireOwnShare(serverId).soundAvailable,
      });
      return Promise.resolve(null);
    },
    screenShareList: (serverId) => {
      const own = ownShares.get(serverId);
      const others = mediaDemo.share !== null && serverId === 1 ? [mediaDemo.share] : [];
      return Promise.resolve((own ? [own] : []).concat(others));
    },
    // The mock cannot send frames, so watching only acknowledges the request; the UI shows
    // `mediaDemo.picture` as the picture. Test data, nothing else.
    screenShareWatch: (_serverId, session) => {
      if (mediaDemo.share === null || mediaDemo.share.session !== session) {
        return notImplemented("screenShareWatch");
      }
      return Promise.resolve(null);
    },
    screenShareBlock: (serverId) => Promise.resolve(shareStartBlock(serverId)),
    screenShareUnwatch: () => notImplemented("screenShareUnwatch"),
    screenShareFramesAttach: (_serverId, session) => {
      if (mediaDemo.share === null || mediaDemo.share.session !== session) {
        return notImplemented("screenShareFramesAttach");
      }
      return Promise.resolve(null);
    },
    screenShareSetDecodeFallback: () => notImplemented("screenShareSetDecodeFallback"),
    screenShareRequestKeyframe: () => notImplemented("screenShareRequestKeyframe"),
    updateStatus: () => Promise.resolve(updater.status()),
    updateCheckNow: () => Promise.resolve(updater.checkNow()),
    updateDownload: () => {
      updater.download();
      return Promise.resolve(null);
    },
    updateCancel: () => {
      updater.cancel();
      return Promise.resolve();
    },
    updateRestart: () => {
      updater.restart();
      return Promise.resolve(null);
    },
    appimageMenuEntryAdd: () => Promise.resolve(null),
    // Nobody has the removed window overlay switched on in the browser mock.
    windowOverlayNoticePending: () => Promise.resolve(false),
    windowOverlayNoticeDismiss: () => Promise.resolve(null),
    audioDeviceNoticePending: () => Promise.resolve(false),
    audioDeviceNoticeDismiss: () => Promise.resolve(null),
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
    requirePassword: (serverId, password) => {
      requiredPasswords.set(serverId, password);
    },
    requireCertTrust: (serverId, previousFingerprint) => {
      certPrompts.set(serverId, previousFingerprint ?? null);
    },
    offerHistory: (channelId, holderName, lines) => {
      offeredHistory.set(channelId, { holderName, lines });
    },
    receivePmRequest: (senderSession, { chatRequest, note, messages = [] }) => {
      focused();
      const sender = requireUser(senderSession);
      const request: MessageRequest = {
        id: nextPmRequestId++,
        session: senderSession,
        name: sender.name,
        certHash: sender.certHash,
        chatRequest,
        note,
        messages: messages.map((body): ChatMessage => ({
          id: String(nextMessageId++),
          conversation:
            sender.certHash === null
              ? { kind: "privateSession", session: senderSession }
              : { kind: "private", certHash: sender.certHash },
          senderName: sender.name,
          senderCertHash: sender.certHash,
          senderSession,
          own: false,
          sentAt: new Date().toISOString(),
          body,
          dismissedPreviews: [],
          syncedFrom: null,
        })),
        receivedAt: new Date().toISOString(),
      };
      const requests = pmRequestsOf(currentServerId);
      setPmRequests(currentServerId, { ...requests, incoming: [...requests.incoming, request] });
      return request;
    },
    receiveMessage: (conversation, body, senderSession) => {
      focused();
      const sender = requireUser(senderSession);
      // Like the engine (issue #140): private messages of blocked people are dropped, their
      // channel messages arrive without unread count.
      const blocked = isBlocked(currentServerId, sender);
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
        syncedFrom: null,
      };
      if (blocked && conversation.kind !== "channel") return message;
      store(message);
      emitServer("messageReceived", { message });
      if (blocked) return message;
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
      focused();
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
      focused();
      if (session()?.state.state !== "reconnecting") return;
      const snap = requireSnapshot();
      setState({ state: "connected", serverId: snap.serverId });
      emit("serverSynced", { snapshot: snap });
    },
    setWhisperKeysUnavailable: (unavailable) => {
      whisperKeysUnavailable = unavailable;
    },
    setIncomingWhisper: (session, kind) => {
      emitServer("incomingWhisperChanged", { session, kind });
    },
    setTalking: (session, talking) => {
      focused();
      emitServer("talkStateChanged", { session, talking });
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
    setSystemDnd: (system) => {
      void setDnd({ ...dnd, system });
    },
    desktopNotices: () => desktopNotices,
    messageSounds: () => messageSounds,
    removeUser: (session) => {
      focused();
      const snap = requireSnapshot();
      snap.users = snap.users.filter((u) => u.session !== session);
      emitServer("userRemoved", { session });
    },
    upsertUser: (user) => {
      focused();
      const snap = requireSnapshot();
      snap.users = [...snap.users.filter((u) => u.session !== user.session), user];
      emitServer("userUpserted", { user });
    },
    lockChannel: (channelId) => {
      focused();
      const snap = requireSnapshot();
      const channel = requireChannel(channelId);
      const locked = {
        ...channel,
        enterRestricted: true,
        permissions: { ...channel.permissions, enter: false },
      };
      snap.channels = snap.channels.map((c) => (c.id === channelId ? locked : c));
      emitServer("channelUpserted", { channel: locked });
    },
    updateChannel: (channelId, patch) => {
      focused();
      const snap = requireSnapshot();
      const updated = { ...requireChannel(channelId), ...patch };
      snap.channels = snap.channels.map((c) => (c.id === channelId ? updated : c));
      emitServer("channelUpserted", { channel: updated });
    },
    renameChannel: (channelId, name) => {
      focused();
      const snap = requireSnapshot();
      const renamed = { ...requireChannel(channelId), name };
      snap.channels = snap.channels.map((c) => (c.id === channelId ? renamed : c));
      emitServer("channelUpserted", { channel: renamed });
    },
    limitChannel: (channelId, maxUsers) => {
      focused();
      const snap = requireSnapshot();
      const limited = { ...requireChannel(channelId), maxUsers };
      snap.channels = snap.channels.map((c) => (c.id === channelId ? limited : c));
      emitServer("channelUpserted", { channel: limited });
    },
    addBareChannelPassword: (serverId, password) => {
      const token = { token: password, label: null, channels: [], lastUsedAt: null };
      channelTokens.set(serverId, [...(channelTokens.get(serverId) ?? []), token]);
    },
    focusServer: (serverId) => {
      focusedServerId = serverId;
    },
  };

  const api: MockApi = { ...guarded, kind: "mock", on, mock, mediaDemo };
  // The browser mock exposes itself for the measuring script and for the mock screenshots.
  if (typeof window !== "undefined") window.__mumcordMock = api;
  return api;
}

/** Pinned channel info (FR-1.48b): demo descriptions and the text behind a hash-only one. */
const noDescription = { description: null, hasDescription: false, maxUsers: null };

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
  [4, "<p>Ranked queue for <b>Squad A</b>. Strategy notes are pinned in #Gaming.</p>"],
]);
