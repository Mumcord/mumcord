/**
 * Plugins in the chat (FR-6.43–6.45): where each plugin chat card sits in the timeline, which
 * text lines a card replaces, and the slash commands of the running plugins. The card trees
 * themselves live in the plugin UI store; this store only decides where they show.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type ChatMessage,
  type MumcordApi,
  type PluginCommand,
  type PluginUi,
  type Unsubscribe,
} from "../../../api";
import { conversationMessages, renderKey, sendingMessages } from "../../chat/chatStore";
import { getServerStateOf, getVoiceServerId, selfUser } from "../../channels/serverStore";

/** A plugin chat card as a local chat item (not a Mumble message). */
export interface PlacedCard {
  plugin: string;
  card: string;
  serverId: number;
  channelId: number;
  /**
   * The message the card follows; `null` when the channel had none loaded. An own message still
   * being sent is named by its local id, which stays its render key once it is confirmed.
   */
  after: string | null;
}

/** What a plugin shows instead of a text line (FR-6.45): its card, or nothing (`card` null). */
export interface CardRef {
  plugin: string;
  card: string | null;
}

export interface PluginChatState {
  /** By {@link cardKey}, oldest first. */
  readonly placed: ReadonlyMap<string, PlacedCard>;
  /** Message id → the card that replaces it, or the plugin that hid it. */
  readonly superseded: ReadonlyMap<string, CardRef>;
  /** Cards that replace a message, by {@link cardKey}; they show there, not as local items. */
  readonly replacing: ReadonlySet<string>;
  /** Commands of the running plugins, owned and taken. */
  readonly commands: readonly PluginCommand[];
  /** Installed plugin id → name (card labels, "via" labels). */
  readonly names: ReadonlyMap<string, string>;
}

/** Cards and replaced lines are kept for the session; old ones go first beyond this. */
const MAX_KEPT = 200;

const EMPTY: PluginChatState = {
  placed: new Map(),
  superseded: new Map(),
  replacing: new Set(),
  commands: [],
  names: new Map(),
};

export function cardKey(plugin: string, card: string): string {
  return `${plugin}\u0000${card}`;
}

function capped<K, V>(map: Map<K, V>): Map<K, V> {
  for (const key of map.keys()) {
    if (map.size <= MAX_KEPT) break;
    map.delete(key);
  }
  return map;
}

/**
 * Where a card that just appeared belongs: the user's channel on `serverId` (the server of the
 * chat event the plugin answered; `null`: the voice server), after its last message. That includes an own message still being sent: the engine hands the user's own message
 * to plugins before the send returns, so a card for it (e.g. a pasted EVE fit) may come first.
 */
function placeCard(plugin: string, card: string, server: number | null): PlacedCard | null {
  const serverId = server ?? getVoiceServerId();
  if (serverId === null) return null;
  const channelId = selfUser(getServerStateOf(serverId))?.channelId;
  if (channelId === undefined) return null;
  const conversation = { kind: "channel", channelId } as const;
  const last =
    sendingMessages(conversation, serverId).at(-1) ??
    conversationMessages(conversation, serverId).at(-1);
  return { plugin, card, serverId, channelId, after: last?.id ?? null };
}

/** Applies a plugin UI change: a new chat card is placed where the user is now on its server. */
export function applyUi(
  state: PluginChatState,
  { plugin, change }: PluginUi,
  place: (plugin: string, card: string, serverId: number | null) => PlacedCard | null = placeCard,
): PluginChatState {
  if (change.type !== "setTree" || change.slot.kind !== "chatCard") return state;
  const key = cardKey(plugin, change.slot.id);
  if (state.placed.has(key)) return state;
  const placed = place(plugin, change.slot.id, change.serverId);
  if (placed === null) return state;
  return { ...state, placed: capped(new Map(state.placed).set(key, placed)) };
}

/** A card replaces a text line, or a plugin hides it (`card` null; `plugin://supersede`). */
export function applySupersede(
  state: PluginChatState,
  plugin: string,
  card: string | null,
  messageId: string,
): PluginChatState {
  return {
    ...state,
    superseded: capped(new Map(state.superseded).set(messageId, { plugin, card })),
    replacing:
      card === null ? state.replacing : new Set(state.replacing).add(cardKey(plugin, card)),
  };
}

/**
 * Lets the plugins see the stored messages of a channel again, render only, so old messages get
 * their cards too (also ones posted before a plugin was installed). Cheap to repeat: the host
 * sends each message to a plugin at most once.
 */
export function replayChannel(serverId: number, channelId: number): void {
  getApi()
    .pluginsReplayChat(serverId, channelId)
    .catch((error: unknown) => {
      console.error("Replaying the chat for plugins failed", error);
    });
}

let state: PluginChatState = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribe: Unsubscribe[] = [];
let load = 0;
const listeners = new Set<() => void>();

function publish(next: PluginChatState): void {
  if (next === state) return;
  state = next;
  for (const listener of listeners) listener();
}

/** Commands and names change only with the installed plugins (`plugins://changed`). */
function loadPlugins(api: MumcordApi): void {
  const mine = ++load;
  Promise.all([api.pluginCommands(), api.pluginsList()]).then(
    ([commands, overview]) => {
      if (mine !== load) return;
      const names = new Map(overview.plugins.map((p) => [p.id, p.name]));
      publish({ ...state, commands, names });
    },
    (error: unknown) => {
      console.error("Loading the plugin commands failed", error);
    },
  );
}

function bind(api: MumcordApi): void {
  for (const off of unsubscribe) off();
  boundApi = api;
  state = EMPTY;
  unsubscribe = [
    api.on("pluginUi", (change) => {
      publish(applyUi(state, change));
    }),
    api.on("pluginSupersede", ({ plugin, card, messageId }) => {
      publish(applySupersede(state, plugin, card, messageId));
    }),
    api.on("pluginsChanged", () => {
      loadPlugins(api);
    }),
  ];
  loadPlugins(api);
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): PluginChatState {
  ensureBound();
  return state;
}

export function usePluginChat(): PluginChatState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** Starts listening without a component (e.g. before the chat opens). */
export function startPluginChat(): void {
  ensureBound();
}

/** For tests. */
export function resetPluginChatStore(): void {
  for (const off of unsubscribe) off();
  unsubscribe = [];
  boundApi = undefined;
  state = EMPTY;
  load++;
}

/** Commands offered in the chat box: those the plugin owns, matching the typed prefix. */
export function matchCommands(commands: readonly PluginCommand[], prefix: string): PluginCommand[] {
  return commands.filter((c) => c.takenBy === null && c.name.startsWith(prefix));
}

/** The owned command a typed line starts with, if any (`/roll 2d6` → roll). */
export function commandOf(
  commands: readonly PluginCommand[],
  line: string,
): PluginCommand | undefined {
  const match = /^\/([a-z0-9-]+)(?:\s|$)/.exec(line);
  const name = match?.[1];
  return name === undefined
    ? undefined
    : commands.find((c) => c.takenBy === null && c.name === name);
}

/** The cards placed right after `message`, in the order they appeared. */
export function cardsAfter(state: PluginChatState, message: ChatMessage): PlacedCard[] {
  return [...state.placed.values()].filter(
    (p) =>
      (p.after === message.id || p.after === renderKey(message.id)) &&
      !state.replacing.has(cardKey(p.plugin, p.card)),
  );
}

/** Cards in a channel that had no message yet when they appeared. */
export function leadingCards(
  state: PluginChatState,
  serverId: number,
  channelId: number,
): PlacedCard[] {
  return [...state.placed.values()].filter(
    (p) =>
      p.after === null &&
      p.serverId === serverId &&
      p.channelId === channelId &&
      !state.replacing.has(cardKey(p.plugin, p.card)),
  );
}

/**
 * The plugin a line was posted by ("[Poll] …" → Poll, FR-6.44), if an installed plugin has that
 * name. The prefix is visible text anyway, so the label claims nothing the line does not.
 */
export function viaPlugin(names: ReadonlyMap<string, string>, body: string): string | null {
  const match = /^\[([^\]<>]{1,32})\] /.exec(body);
  const name = match?.[1];
  if (name === undefined) return null;
  for (const installed of names.values()) if (installed === name) return name;
  return null;
}

/** The channel of a chat list key (`<serverId>:<conversation JSON>`, see ChatView). */
export function channelOfListKey(listKey: string): { serverId: number; channelId: number } | null {
  const split = listKey.indexOf(":");
  if (split < 0) return null;
  try {
    const conversation = JSON.parse(listKey.slice(split + 1)) as unknown;
    if (
      typeof conversation === "object" &&
      conversation !== null &&
      "kind" in conversation &&
      conversation.kind === "channel" &&
      "channelId" in conversation &&
      typeof conversation.channelId === "number"
    ) {
      return { serverId: Number(listKey.slice(0, split)), channelId: conversation.channelId };
    }
  } catch {
    // Not a channel key: no leading cards.
  }
  return null;
}
