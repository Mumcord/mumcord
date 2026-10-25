/**
 * Messages per conversation for the current connection: live `messageReceived` events, own sent
 * messages, history pages from `historyLoad` (scroll-back) and messages synced from other
 * Mumcord users' history (`historySynced`, inserted at their time), de-duplicated by id.
 * Cleared on every `serverSynced` and on disconnect, because channel and session ids are only
 * valid for one connection; the history is then reloaded from the engine.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type ChatMessage,
  type Conversation,
  type MumcordApi,
  type Unsubscribe,
} from "../../api";

export const HISTORY_PAGE = 50;
/** Live messages kept per conversation; older ones are dropped and can be paged in again. */
export const MAX_KEPT = 500;

export interface ConversationLog {
  /** Oldest first. */
  messages: readonly ChatMessage[];
  /** The newest history page has been requested and arrived (or failed). */
  loaded: boolean;
  loading: boolean;
  /** More history may exist before the first message. */
  hasMore: boolean;
  loadFailed: boolean;
  /** A history sync of this channel with other Mumcord users is running (FR-1.60). */
  syncing: boolean;
  /** Messages added by the last sync the user asked for; `null` while none finished. */
  syncResult: number | null;
  /** The last sync the user asked for could not start. */
  syncFailed: boolean;
}

const EMPTY_LOG: ConversationLog = {
  messages: [],
  loaded: false,
  loading: false,
  hasMore: false,
  loadFailed: false,
  syncing: false,
  syncResult: null,
  syncFailed: false,
};

export function conversationKey(conversation: Conversation): string {
  switch (conversation.kind) {
    case "channel":
      return `channel:${String(conversation.channelId)}`;
    case "private":
      return `private:${conversation.certHash}`;
    case "privateSession":
      return `session:${String(conversation.session)}`;
  }
}

let logs = new Map<string, ConversationLog>();
/** Bumped on every reset so late history responses of an old connection are ignored. */
let generation = 0;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();
const incomingListeners = new Set<(message: ChatMessage) => void>();
/** Channels whose running sync the user asked for; only those report their result. */
const syncRequested = new Set<string>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setLog(key: string, log: ConversationLog): void {
  logs = new Map(logs);
  logs.set(key, log);
  notify();
}

function getLog(key: string): ConversationLog {
  return logs.get(key) ?? EMPTY_LOG;
}

function reset(): void {
  generation += 1;
  syncRequested.clear();
  if (logs.size === 0) return;
  logs = new Map();
  notify();
}

/** Merges messages by id, keeping the order oldest first. */
function merge(
  older: readonly ChatMessage[],
  newer: readonly ChatMessage[],
): readonly ChatMessage[] {
  const seen = new Set(older.map((m) => m.id));
  return [...older, ...newer.filter((m) => !seen.has(m.id))];
}

function append(message: ChatMessage): void {
  const key = conversationKey(message.conversation);
  const log = getLog(key);
  if (log.messages.some((m) => m.id === message.id)) return;
  const messages = [...log.messages, message];
  const overflow = messages.length > MAX_KEPT;
  setLog(key, {
    ...log,
    messages: overflow ? messages.slice(messages.length - MAX_KEPT) : messages,
    hasMore: log.hasMore || overflow,
  });
}

/**
 * Inserts messages into a list sorted oldest first, each after every message sent at the same
 * time or earlier. Synced batches arrive sorted by time with ties broken by content hash, so
 * equal times keep the engine's order.
 */
export function insertByTime(
  messages: readonly ChatMessage[],
  incoming: readonly ChatMessage[],
): readonly ChatMessage[] {
  const result = [...messages];
  for (const message of incoming) {
    const time = Date.parse(message.sentAt);
    let at = result.length;
    while (at > 0 && Date.parse(result[at - 1]?.sentAt ?? "") > time) at -= 1;
    result.splice(at, 0, message);
  }
  return result;
}

function channelKey(channelId: number): string {
  return conversationKey({ kind: "channel", channelId });
}

function insertSynced(channelId: number, incoming: readonly ChatMessage[]): void {
  const key = channelKey(channelId);
  const log = getLog(key);
  // Not shown yet: the first history page will contain them (they are already stored).
  if (!log.loaded) return;
  const known = new Set(log.messages.map((m) => m.id));
  const oldest = log.messages[0];
  // Older than the loaded window: paging back brings them in the right place; inserting them
  // now would hide the stored messages in between from paging.
  const fits = incoming.filter(
    (m) =>
      !known.has(m.id) &&
      (!log.hasMore || (oldest !== undefined && Date.parse(m.sentAt) >= Date.parse(oldest.sentAt))),
  );
  if (fits.length === 0) return;
  const messages = insertByTime(log.messages, fits);
  const overflow = messages.length > MAX_KEPT;
  setLog(key, {
    ...log,
    messages: overflow ? messages.slice(messages.length - MAX_KEPT) : messages,
    hasMore: log.hasMore || overflow,
  });
}

function onSyncStatus(channelId: number, running: boolean, added: number): void {
  const key = channelKey(channelId);
  const asked = syncRequested.has(key);
  if (!running) syncRequested.delete(key);
  setLog(key, {
    ...getLog(key),
    syncing: running,
    syncResult: !running && asked ? added : null,
  });
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  logs = new Map();
  generation += 1;
  unsubscribers = [
    api.on("messageReceived", ({ message }) => {
      append(message);
      for (const listener of incomingListeners) listener(message);
    }),
    api.on("historySynced", ({ channelId, messages }) => {
      insertSynced(channelId, messages);
    }),
    api.on("historySyncStatus", ({ channelId, running, added }) => {
      onSyncStatus(channelId, running, added);
    }),
    api.on("serverSynced", reset),
    api.on("connectionStateChanged", ({ state }) => {
      if (state.state === "disconnected") reset();
    }),
  ];
}

function ensureBound(): MumcordApi {
  const api = getApi();
  if (api !== boundApi) bind(api);
  return api;
}

async function fetchPage(conversation: Conversation, older: boolean): Promise<void> {
  const api = ensureBound();
  const key = conversationKey(conversation);
  const log = getLog(key);
  if (log.loading) return;
  const before = older ? (log.messages[0]?.id ?? null) : null;
  const started = generation;
  setLog(key, { ...log, loading: true, loadFailed: false });
  try {
    const page = await api.historyLoad(conversation, before, HISTORY_PAGE);
    if (started !== generation) return;
    const current = getLog(key);
    setLog(key, {
      ...current,
      messages: merge(page, current.messages),
      loaded: true,
      loading: false,
      hasMore: page.length === HISTORY_PAGE,
      loadFailed: false,
    });
  } catch (error) {
    if (started !== generation) return;
    console.error("Loading chat history failed", error);
    setLog(key, { ...getLog(key), loaded: true, loading: false, loadFailed: true });
  }
}

/** Loads the newest history page once per conversation and connection. */
export function ensureLoaded(conversation: Conversation): void {
  ensureBound();
  const log = getLog(conversationKey(conversation));
  if (!log.loaded && !log.loading) void fetchPage(conversation, false);
}

/** Pages in older history before the first loaded message. */
export function loadOlder(conversation: Conversation): Promise<void> {
  return fetchPage(conversation, true);
}

/**
 * "Load earlier messages" beyond the local history (FR-1.60): asks other Mumcord users for the
 * channel's earlier messages. Progress and the result show in the log (`syncing`,
 * `syncResult`, `syncFailed`); the messages arrive as `historySynced`.
 */
export async function requestSync(conversation: Conversation): Promise<void> {
  if (conversation.kind !== "channel") return;
  const api = ensureBound();
  const key = conversationKey(conversation);
  if (getLog(key).syncing) return;
  syncRequested.add(key);
  setLog(key, { ...getLog(key), syncResult: null, syncFailed: false });
  try {
    await api.historySyncRequest(conversation.channelId);
  } catch (error) {
    console.error("Requesting earlier messages failed", error);
    syncRequested.delete(key);
    setLog(key, { ...getLog(key), syncFailed: true });
  }
}

/** Adds an own sent message (the engine returns it instead of emitting an event). */
export function addSentMessage(message: ChatMessage): void {
  ensureBound();
  append(message);
}

/** Forgets a conversation's messages, e.g. after its history was cleared (B22). */
export function clearConversation(conversation: Conversation): void {
  const key = conversationKey(conversation);
  if (!logs.has(key)) return;
  logs = new Map(logs);
  logs.delete(key);
  notify();
}

/** Called for every incoming message of any conversation. */
export function subscribeIncoming(listener: (message: ChatMessage) => void): () => void {
  ensureBound();
  incomingListeners.add(listener);
  return () => {
    incomingListeners.delete(listener);
  };
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getLogs(): ReadonlyMap<string, ConversationLog> {
  ensureBound();
  return logs;
}

/** Messages and loading state of a conversation; re-renders on change. */
export function useConversationLog(conversation: Conversation | null): ConversationLog {
  const all = useSyncExternalStore(subscribe, getLogs);
  return conversation ? (all.get(conversationKey(conversation)) ?? EMPTY_LOG) : EMPTY_LOG;
}

/** Test helper: drops the binding so the next use binds to the then-active client. */
export function resetChatStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  logs = new Map();
  generation += 1;
  incomingListeners.clear();
  syncRequested.clear();
}

/** Only the messages of a conversation, oldest first (contract for B22). */
export function useConversationMessages(conversation: Conversation | null): readonly ChatMessage[] {
  return useConversationLog(conversation).messages;
}
