/**
 * Followed channels, unread counts and the list of open private chats of the connected server.
 * One store for the app; it binds to the active engine client and re-binds when tests swap it.
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

let state: FollowState = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
let generation = 0;
const listeners = new Set<() => void>();

function set(next: FollowState): void {
  state = next;
  for (const listener of listeners) listener();
}

function isPrivate(conversation: Conversation): boolean {
  return conversation.kind !== "channel";
}

/** Remembers a private chat in the "Direct messages" list. */
export function rememberDirectChat(conversation: Conversation, name: string | null): void {
  if (!isPrivate(conversation)) return;
  const key = conversationKey(conversation);
  const existing = state.chats.find((c) => c.key === key);
  if (existing && (name === null || existing.name === name)) return;
  const chat: DirectChat = { conversation, key, name: name ?? existing?.name ?? null };
  set({ ...state, chats: [...state.chats.filter((c) => c.key !== key), chat] });
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

function onMessage(message: ChatMessage): void {
  if (!isPrivate(message.conversation)) return;
  rememberDirectChat(message.conversation, message.own ? null : message.senderName);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  state = EMPTY;
  const mine = ++generation;
  unsubscribers = [
    api.on("serverSynced", ({ snapshot }) => {
      set(fromSnapshot(snapshot));
    }),
    api.on("connectionStateChanged", ({ state: connection }) => {
      if (connection.state === "disconnected" || connection.state === "connecting") set(EMPTY);
    }),
    api.on("followedChannelsChanged", ({ channelIds }) => {
      set({ ...state, followed: channelIds });
    }),
    api.on("unreadChanged", ({ conversation, count }) => {
      const unread = new Map(state.unread);
      unread.set(conversationKey(conversation), count);
      set({ ...state, unread });
      if (count > 0) rememberDirectChat(conversation, null);
    }),
    api.on("messageReceived", ({ message }) => {
      onMessage(message);
    }),
  ];
  void api
    .sessionSnapshot()
    .then((snapshot) => {
      if (generation !== mine || !snapshot) return;
      if (state.followed.length === 0 && state.unread.size === 0 && state.chats.length === 0) {
        set(fromSnapshot(snapshot));
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

function getSnapshot(): FollowState {
  ensureBound();
  return state;
}

export function useFollowState(): FollowState {
  return useSyncExternalStore(subscribe, getSnapshot);
}

export function getFollowState(): FollowState {
  return getSnapshot();
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
  state = EMPTY;
}
