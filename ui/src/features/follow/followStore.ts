/**
 * Followed channels, unread counts and the list of open private chats, per connected server.
 * One store for the app; it binds to the active engine client and re-binds when tests swap it.
 * Functions without a `serverId` act on the shown server.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type ChatMessage,
  type Conversation,
  type MumcordApi,
  type ServerSnapshot,
  type Unsubscribe,
} from "../../api";
import { getShownServerId, shownServerId, useShownServerId } from "../channels/serverStore";
import { conversationKey } from "../chat/chatStore";

export interface DirectChat {
  conversation: Conversation;
  key: string;
  /** Name known when the chat was opened; the live name of the person wins while they are online. */
  name: string | null;
}

export interface FollowState {
  followed: readonly number[];
  unread: ReadonlyMap<string, number>;
  chats: readonly DirectChat[];
}

const EMPTY: FollowState = { followed: [], unread: new Map(), chats: [] };

let states: ReadonlyMap<number, FollowState> = new Map();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
let generation = 0;
const listeners = new Set<() => void>();

/** Conversations currently shown in the chat column; their messages are read on arrival. */
const onScreen = new Map<string, Conversation>();

/** Marks a conversation as shown (or no longer shown) so incoming messages never raise its badge. */
export function setConversationOnScreen(conversation: Conversation, shown: boolean): void {
  const key = conversationKey(conversation);
  if (shown) onScreen.set(key, conversation);
  else onScreen.delete(key);
}

function stateOf(serverId: number): FollowState {
  return states.get(serverId) ?? EMPTY;
}

function set(serverId: number, next: FollowState): void {
  const copy = new Map(states);
  if (next === EMPTY) copy.delete(serverId);
  else copy.set(serverId, next);
  states = copy;
  for (const listener of listeners) listener();
}

function isPrivate(conversation: Conversation): boolean {
  return conversation.kind !== "channel";
}

/** Remembers a private chat in the "Direct messages" list of `serverId`. */
export function rememberDirectChat(
  conversation: Conversation,
  name: string | null,
  serverId = shownServerId(),
): void {
  if (!isPrivate(conversation)) return;
  const state = stateOf(serverId);
  const key = conversationKey(conversation);
  const existing = state.chats.find((c) => c.key === key);
  if (existing && (name === null || existing.name === name)) return;
  const chat: DirectChat = { conversation, key, name: name ?? existing?.name ?? null };
  set(serverId, { ...state, chats: [...state.chats.filter((c) => c.key !== key), chat] });
}

function fromSnapshot(snapshot: ServerSnapshot): FollowState {
  const unread = new Map(snapshot.unread.map((u) => [conversationKey(u.conversation), u.count]));
  const chats = snapshot.unread
    .filter((u) => isPrivate(u.conversation))
    .map((u) => ({
      conversation: u.conversation,
      key: conversationKey(u.conversation),
      name: null,
    }));
  return { followed: snapshot.followedChannelIds, unread, chats };
}

function onMessage(serverId: number, message: ChatMessage): void {
  if (!isPrivate(message.conversation)) return;
  rememberDirectChat(message.conversation, message.own ? null : message.senderName, serverId);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  states = new Map();
  const mine = ++generation;
  unsubscribers = [
    api.on("serverSynced", ({ snapshot }) => {
      set(snapshot.serverId, fromSnapshot(snapshot));
    }),
    api.on("connectionStateChanged", ({ serverId, state: connection }) => {
      if (connection.state === "disconnected" || connection.state === "connecting") {
        set(serverId, EMPTY);
      }
    }),
    api.on("followedChannelsChanged", ({ serverId, channelIds }) => {
      set(serverId, { ...stateOf(serverId), followed: channelIds });
    }),
    api.on("unreadChanged", ({ serverId, conversation, count }) => {
      // Dropped before the store changes: rendering the count first would flash the badge until
      // the mark-read round trip comes back.
      const key = conversationKey(conversation);
      if (count > 0 && serverId === getShownServerId() && onScreen.has(key)) {
        api.conversationMarkRead(serverId, conversation).catch((error: unknown) => {
          console.error("Marking a conversation as read failed", error);
        });
        return;
      }
      const state = stateOf(serverId);
      const unread = new Map(state.unread);
      unread.set(key, count);
      set(serverId, { ...state, unread });
      if (count > 0) rememberDirectChat(conversation, null, serverId);
    }),
    api.on("messageReceived", ({ serverId, message }) => {
      onMessage(serverId, message);
    }),
  ];
  // Read the engine's own list: the server store may not know the servers yet at this point.
  void api
    .connectionsList()
    .then((list) => Promise.all(list.map((entry) => api.sessionSnapshot(entry.serverId))))
    .then((snapshots) => {
      if (generation !== mine) return;
      for (const snapshot of snapshots) {
        if (snapshot && !states.has(snapshot.serverId)) {
          set(snapshot.serverId, fromSnapshot(snapshot));
        }
      }
    })
    .catch((error: unknown) => {
      console.error("Loading followed channels failed", error);
    });
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

function getStates(): ReadonlyMap<number, FollowState> {
  ensureBound();
  return states;
}

/** Follow state of every connected server, by server id (e.g. unread badges per server). */
export function useFollowStates(): ReadonlyMap<number, FollowState> {
  return useSyncExternalStore(subscribe, getStates);
}

/** Follow state of `serverId`, by default the shown server. */
export function useFollowState(serverId?: number | null): FollowState {
  const shown = useShownServerId();
  const all = useFollowStates();
  const server = serverId === undefined ? shown : serverId;
  return (server === null ? undefined : all.get(server)) ?? EMPTY;
}

export function getFollowState(serverId = shownServerId()): FollowState {
  return getStates().get(serverId) ?? EMPTY;
}

export function unreadOf(s: FollowState, conversation: Conversation): number {
  return s.unread.get(conversationKey(conversation)) ?? 0;
}

/** Test helper. */
export function resetFollowStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  generation += 1;
  states = new Map();
}
