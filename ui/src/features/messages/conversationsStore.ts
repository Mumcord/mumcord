/**
 * Data of the private messages overview (issue #173): the newest message of every private
 * conversation per connected server, and the saved pin and mute state. The newest messages come
 * from the stored history when a server syncs and follow every message received or sent; they are
 * dropped on disconnect. One store for the app; it binds to the active engine client and re-binds
 * when tests swap it.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type ChatMessage,
  type Conversation,
  type MumcordApi,
  type PmConversationPrefs,
  type Unsubscribe,
} from "../../api";
import { conversationKey, subscribeSent } from "../chat";
import { hasImage, plainPreview } from "../notifications/preview";

/** The newest message of one private conversation, as plain text. */
export interface LastMessage {
  conversation: Conversation;
  /** Name the other person last wrote under; untrusted plain text. */
  peerName: string | null;
  /** Plain-text preview; empty for a message that is only an image. */
  text: string;
  image: boolean;
  own: boolean;
  /** Unix milliseconds. */
  at: number;
}

export interface ConversationsState {
  /** By server id, then by conversation key. */
  last: ReadonlyMap<number, ReadonlyMap<string, LastMessage>>;
  prefs: readonly PmConversationPrefs[];
}

let state: ConversationsState = { last: new Map(), prefs: [] };
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
let generation = 0;
/** Bumped by every change of a server's messages, so a load started earlier cannot overwrite it. */
const versions = new Map<number, number>();
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setServer(serverId: number, entries: ReadonlyMap<string, LastMessage> | null): void {
  versions.set(serverId, (versions.get(serverId) ?? 0) + 1);
  const last = new Map(state.last);
  if (entries === null) last.delete(serverId);
  else last.set(serverId, entries);
  state = { ...state, last };
  notify();
}

function toLast(message: ChatMessage, peerName: string | null): LastMessage {
  return {
    conversation: message.conversation,
    peerName,
    text: plainPreview(message.body),
    image: hasImage(message.body),
    own: message.own,
    at: Date.parse(message.sentAt),
  };
}

/** Keeps `message` as the newest of its conversation unless a newer one is already known. */
function record(serverId: number, message: ChatMessage): void {
  if (message.conversation.kind === "channel") return;
  const key = conversationKey(message.conversation);
  const server = state.last.get(serverId) ?? new Map<string, LastMessage>();
  const old = server.get(key);
  const next = toLast(message, message.own ? (old?.peerName ?? null) : message.senderName);
  if (old && old.at > next.at) return;
  setServer(serverId, new Map(server).set(key, next));
}

function load(api: MumcordApi, serverId: number): void {
  const mine = generation;
  const version = versions.get(serverId) ?? 0;
  api.privateConversations(serverId).then(
    (list) => {
      if (generation !== mine || (versions.get(serverId) ?? 0) !== version) return;
      const entries = new Map<string, LastMessage>();
      for (const c of list)
        entries.set(conversationKey(c.last.conversation), toLast(c.last, c.peerName));
      setServer(serverId, entries);
    },
    (error: unknown) => {
      console.error("Loading the private conversations failed", error);
    },
  );
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  state = { last: new Map(), prefs: [] };
  versions.clear();
  const mine = ++generation;
  unsubscribers = [
    api.on("serverSynced", ({ snapshot }) => {
      load(api, snapshot.serverId);
    }),
    api.on("connectionStateChanged", ({ serverId, state: connection }) => {
      if (connection.state === "disconnected") setServer(serverId, null);
    }),
    api.on("messageReceived", ({ serverId, message }) => {
      record(serverId, message);
    }),
    api.on("pmConversationPrefsChanged", ({ prefs }) => {
      state = { ...state, prefs };
      notify();
    }),
    subscribeSent((message, serverId) => {
      record(serverId, message);
    }),
  ];
  api.pmConversationPrefs().then(
    (prefs) => {
      if (generation !== mine) return;
      state = { ...state, prefs };
      notify();
    },
    (error: unknown) => {
      console.error("Loading pinned and muted conversations failed", error);
    },
  );
  // Servers connected before the overview was first shown.
  api.connectionsList().then(
    (list) => {
      if (generation !== mine) return;
      for (const entry of list) if (!state.last.has(entry.serverId)) load(api, entry.serverId);
    },
    (error: unknown) => {
      console.error("Loading the private conversations failed", error);
    },
  );
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

function getState(): ConversationsState {
  ensureBound();
  return state;
}

export function useConversationsState(): ConversationsState {
  return useSyncExternalStore(subscribe, getState);
}

/** Drops a conversation after its history was deleted on this device. */
export function forgetConversation(serverId: number, conversation: Conversation): void {
  const server = state.last.get(serverId);
  const key = conversationKey(conversation);
  if (!server?.has(key)) return;
  const next = new Map(server);
  next.delete(key);
  setServer(serverId, next);
}

/** Whether the conversation with `certHash` on `serverId` is muted at `now` (Unix millis). */
export function isMuted(
  prefs: readonly PmConversationPrefs[],
  serverId: number,
  certHash: string | null,
  now: number,
): boolean {
  if (certHash === null) return false;
  const p = prefs.find((x) => x.serverId === serverId && x.certHash === certHash);
  if (!p) return false;
  return p.mutedForever || (p.mutedUntil !== null && Date.parse(p.mutedUntil) > now);
}

/** Whether new messages of `conversation` should stay quiet (no toast, no sound). */
export function isConversationMuted(serverId: number, conversation: Conversation): boolean {
  if (conversation.kind !== "private") return false;
  return isMuted(getState().prefs, serverId, conversation.certHash, Date.now());
}

/** Test helper. */
export function resetConversationsStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  generation += 1;
  versions.clear();
  state = { last: new Map(), prefs: [] };
}
