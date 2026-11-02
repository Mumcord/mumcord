/**
 * Messages per server and conversation for the current connections: live `messageReceived`
 * events, own sent messages, history pages from `historyLoad` (scroll-back) and messages synced
 * from other Mumcord users' history (`historySynced`, inserted at their time), de-duplicated by
 * id. A server's messages are cleared on its every `serverSynced` and on its disconnect, because
 * channel and session ids are only valid for one connection; the history is then reloaded from
 * the engine. Functions without a `serverId` act on the shown server.
 */
import { useCallback, useSyncExternalStore } from "react";
import {
  getApi,
  type ChatMessage,
  type Conversation,
  type MumcordApi,
  type Unsubscribe,
} from "../../api";
import { shownServerId, useShownServerId } from "../channels/serverStore";
import type { SendTarget } from "./conversationInfo";

export const HISTORY_PAGE = 50;
/** Live messages kept per conversation; older ones are dropped and can be paged in again. */
export const MAX_KEPT = 500;

/** Prefix of the local ids of own messages not confirmed yet; they never reach the engine. */
export const OUTGOING_PREFIX = "tmp-out-";

/**
 * An own message on its way (FR-2.74): shown greyed at the end of the conversation until the
 * engine confirms it, or kept with the reason when sending failed (FR-2.47) so it can be retried.
 * Local only: it is never stored while it is not confirmed.
 */
export interface OutgoingSend {
  serverId: number;
  conversation: Conversation;
  target: SendTarget;
  html: string;
  /** How it shows in the list; its id is the local id. */
  message: ChatMessage;
  /** Shown when the server finds it too large, already translated. */
  tooLarge: string;
  /** `null` while sending; the plain-language reason once it failed, already translated. */
  error: string | null;
}

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
  /** Own messages not confirmed yet (sending or failed), oldest first; absent when none. */
  outgoing?: readonly OutgoingSend[];
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

/** Key of a conversation on one server: conversation ids are only unique per server. */
function logKey(serverId: number, conversation: Conversation): string {
  return `${String(serverId)}/${conversationKey(conversation)}`;
}

function serverPrefix(serverId: number): string {
  return `${String(serverId)}/`;
}

let logs = new Map<string, ConversationLog>();
/**
 * Bumped per server on every reset so late history responses of an old connection are ignored.
 * Never deleted, so a reconnect cannot bring an old number back.
 */
const generations = new Map<number, number>();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();
const incomingListeners = new Set<(message: ChatMessage, serverId: number) => void>();
const sentListeners = new Set<(message: ChatMessage, serverId: number) => void>();
/** Channels whose running sync the user asked for; only those report their result. */
const syncRequested = new Set<string>();

function generation(serverId: number): number {
  return generations.get(serverId) ?? 0;
}

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

/**
 * Forgets one server's messages; the other servers keep theirs. Unconfirmed own messages stay so
 * their text is not lost and they can be retried after a reconnect, except those to a person
 * without a certificate: their session id may belong to someone else after the reconnect.
 */
function reset(serverId: number): void {
  generations.set(serverId, generation(serverId) + 1);
  const prefix = serverPrefix(serverId);
  for (const key of syncRequested) if (key.startsWith(prefix)) syncRequested.delete(key);
  const kept = [...logs].flatMap(([key, log]): [string, ConversationLog][] => {
    if (!key.startsWith(prefix)) return [[key, log]];
    const outgoing = (log.outgoing ?? []).filter(
      (item) => item.conversation.kind !== "privateSession",
    );
    return outgoing.length > 0 ? [[key, { ...EMPTY_LOG, outgoing }]] : [];
  });
  if (kept.length === logs.size && kept.every(([key, log]) => logs.get(key) === log)) return;
  logs = new Map(kept);
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

function withMessage(log: ConversationLog, message: ChatMessage): ConversationLog {
  if (log.messages.some((m) => m.id === message.id)) return log;
  const messages = [...log.messages, message];
  const overflow = messages.length > MAX_KEPT;
  return {
    ...log,
    messages: overflow ? messages.slice(messages.length - MAX_KEPT) : messages,
    hasMore: log.hasMore || overflow,
  };
}

function append(serverId: number, message: ChatMessage): void {
  const key = logKey(serverId, message.conversation);
  const log = getLog(key);
  const next = withMessage(log, message);
  if (next !== log) setLog(key, next);
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

function channelKey(serverId: number, channelId: number): string {
  return logKey(serverId, { kind: "channel", channelId });
}

function insertSynced(serverId: number, channelId: number, incoming: readonly ChatMessage[]): void {
  const key = channelKey(serverId, channelId);
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

function onSyncStatus(serverId: number, channelId: number, running: boolean, added: number): void {
  const key = channelKey(serverId, channelId);
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
  bumpAll();
  unsubscribers = [
    api.on("messageReceived", ({ serverId, message }) => {
      append(serverId, message);
      for (const listener of incomingListeners) listener(message, serverId);
    }),
    api.on("historySynced", ({ serverId, channelId, messages }) => {
      insertSynced(serverId, channelId, messages);
    }),
    api.on("historySyncStatus", ({ serverId, channelId, running, added }) => {
      onSyncStatus(serverId, channelId, running, added);
    }),
    api.on("serverSynced", ({ snapshot }) => {
      reset(snapshot.serverId);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state === "disconnected") reset(serverId);
    }),
  ];
}

/** Invalidates the running history requests of every server. */
function bumpAll(): void {
  for (const [serverId, value] of generations) generations.set(serverId, value + 1);
}

function ensureBound(): MumcordApi {
  const api = getApi();
  if (api !== boundApi) bind(api);
  return api;
}

async function fetchPage(
  serverId: number,
  conversation: Conversation,
  older: boolean,
): Promise<void> {
  const api = ensureBound();
  const key = logKey(serverId, conversation);
  const log = getLog(key);
  if (log.loading) return;
  const before = older ? (log.messages[0]?.id ?? null) : null;
  const started = generation(serverId);
  setLog(key, { ...log, loading: true, loadFailed: false });
  try {
    const page = await api.historyLoad(serverId, conversation, before, HISTORY_PAGE);
    if (started !== generation(serverId)) return;
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
    if (started !== generation(serverId)) return;
    console.error("Loading chat history failed", error);
    setLog(key, { ...getLog(key), loaded: true, loading: false, loadFailed: true });
  }
}

/** Loads the newest history page once per conversation and connection. */
export function ensureLoaded(conversation: Conversation, serverId = shownServerId()): void {
  ensureBound();
  const log = getLog(logKey(serverId, conversation));
  if (!log.loaded && !log.loading) void fetchPage(serverId, conversation, false);
}

/** Pages in older history before the first loaded message. */
export function loadOlder(conversation: Conversation, serverId = shownServerId()): Promise<void> {
  return fetchPage(serverId, conversation, true);
}

/**
 * "Load earlier messages" beyond the local history (FR-1.60): asks other Mumcord users for the
 * channel's earlier messages. Progress and the result show in the log (`syncing`,
 * `syncResult`, `syncFailed`); the messages arrive as `historySynced`.
 */
export async function requestSync(
  conversation: Conversation,
  serverId = shownServerId(),
): Promise<void> {
  if (conversation.kind !== "channel") return;
  const api = ensureBound();
  const key = logKey(serverId, conversation);
  if (getLog(key).syncing) return;
  syncRequested.add(key);
  setLog(key, { ...getLog(key), syncResult: null, syncFailed: false });
  try {
    await api.historySyncRequest(serverId, conversation.channelId);
  } catch (error) {
    console.error("Requesting earlier messages failed", error);
    syncRequested.delete(key);
    setLog(key, { ...getLog(key), syncFailed: true });
  }
}

/** Adds an own sent message (the engine returns it instead of emitting an event). */
export function addSentMessage(message: ChatMessage, serverId = shownServerId()): void {
  ensureBound();
  append(serverId, message);
  for (const listener of sentListeners) listener(message, serverId);
}

let nextOutgoingId = 1;
/** Local id of a confirmed own message, by its stored id: its list item stays the same element. */
let renderKeys = new Map<string, string>();

/** The React key of a message: the local id it was shown with while sending, else its id. */
export function renderKey(id: string): string {
  return renderKeys.get(id) ?? id;
}

function setOutgoing(
  serverId: number,
  conversation: Conversation,
  change: (list: readonly OutgoingSend[]) => readonly OutgoingSend[],
): void {
  const key = logKey(serverId, conversation);
  const log = getLog(key);
  setLog(key, { ...log, outgoing: change(log.outgoing ?? []) });
}

/** Shows an own message greyed at the end of its conversation until it is confirmed. */
export function addOutgoing(
  send: Omit<OutgoingSend, "message" | "error">,
  sender: Pick<ChatMessage, "senderName" | "senderCertHash" | "senderSession">,
): OutgoingSend {
  ensureBound();
  const entry: OutgoingSend = {
    ...send,
    message: {
      ...sender,
      id: `${OUTGOING_PREFIX}${String(nextOutgoingId++)}`,
      conversation: send.conversation,
      own: true,
      sentAt: new Date().toISOString(),
      body: send.html,
      dismissedPreviews: [],
      syncedFrom: null,
    },
    error: null,
  };
  setOutgoing(send.serverId, send.conversation, (list) => [...list, entry]);
  return entry;
}

/** Sets the failure reason of an unconfirmed message; `null` while it is sent again. */
export function setOutgoingError(entry: OutgoingSend, error: string | null): void {
  setOutgoing(entry.serverId, entry.conversation, (list) =>
    list.map((item) => (item.message.id === entry.message.id ? { ...item, error } : item)),
  );
}

/** Removes an unconfirmed message (the user deleted it). */
export function removeOutgoing(entry: OutgoingSend): void {
  setOutgoing(entry.serverId, entry.conversation, (list) =>
    list.filter((item) => item.message.id !== entry.message.id),
  );
}

/**
 * Replaces an unconfirmed message by the stored one the engine returned, in one update and under
 * the same React key, so the list neither jumps nor shows it twice.
 */
export function confirmOutgoing(entry: OutgoingSend, sent: ChatMessage): void {
  renderKeys = new Map(renderKeys).set(sent.id, entry.message.id);
  const from = logKey(entry.serverId, entry.conversation);
  const to = logKey(entry.serverId, sent.conversation);
  const log = getLog(from);
  const outgoing = (log.outgoing ?? []).filter((item) => item.message.id !== entry.message.id);
  logs = new Map(logs);
  logs.set(from, { ...log, outgoing });
  logs.set(to, withMessage(getLog(to), sent));
  notify();
  for (const listener of sentListeners) listener(sent, entry.serverId);
}

/** Forgets a conversation's messages, e.g. after its history was cleared (B22). */
export function clearConversation(conversation: Conversation, serverId = shownServerId()): void {
  const key = logKey(serverId, conversation);
  if (!logs.has(key)) return;
  logs = new Map(logs);
  logs.delete(key);
  notify();
}

/** Called for every own message the engine confirmed as sent, on any connected server. */
export function subscribeSent(
  listener: (message: ChatMessage, serverId: number) => void,
): () => void {
  ensureBound();
  sentListeners.add(listener);
  return () => {
    sentListeners.delete(listener);
  };
}

/** Called for every incoming message of any conversation on any connected server. */
export function subscribeIncoming(
  listener: (message: ChatMessage, serverId: number) => void,
): () => void {
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

/** The loaded messages of a conversation on `serverId`, oldest first. */
export function conversationMessages(
  conversation: Conversation,
  serverId: number,
): readonly ChatMessage[] {
  return (getLogs().get(logKey(serverId, conversation)) ?? EMPTY_LOG).messages;
}

/** Own messages of a conversation on `serverId` still being sent (not failed), oldest first. */
export function sendingMessages(
  conversation: Conversation,
  serverId: number,
): readonly ChatMessage[] {
  const outgoing = getLogs().get(logKey(serverId, conversation))?.outgoing ?? [];
  return outgoing.filter((item) => item.error === null).map((item) => item.message);
}

/**
 * Messages and loading state of a conversation on `serverId` (default: the shown server);
 * re-renders on change.
 */
export function useConversationLog(
  conversation: Conversation | null,
  serverId?: number | null,
): ConversationLog {
  const shown = useShownServerId();
  const server = serverId === undefined ? shown : serverId;
  const key = conversation && server !== null ? logKey(server, conversation) : null;
  const get = useCallback(
    () => (key === null ? EMPTY_LOG : (getLogs().get(key) ?? EMPTY_LOG)),
    [key],
  );
  return useSyncExternalStore(subscribe, get);
}

/** Test helper: drops the binding so the next use binds to the then-active client. */
export function resetChatStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  logs = new Map();
  renderKeys = new Map();
  bumpAll();
  incomingListeners.clear();
  sentListeners.clear();
  syncRequested.clear();
}

/** Only the messages of a conversation, oldest first (contract for B22). */
export function useConversationMessages(
  conversation: Conversation | null,
  serverId?: number | null,
): readonly ChatMessage[] {
  return useConversationLog(conversation, serverId).messages;
}
