/**
 * The conversation the chat column shows ("selected conversation"). Shared by the channel tree
 * (single click), the member list (send private message), chat (B21) and follow/PMs (B22).
 *
 * - After each `serverSynced` it starts at the own channel; on disconnect it becomes `null`.
 * - If the selected channel is removed it falls back to the own channel.
 * - It belongs to the shown server: showing another server brings back what was selected there
 *   (FR-2.5), until that server disconnects.
 */
import { useCallback, useSyncExternalStore } from "react";
import type { Conversation } from "../../api";
import {
  getServerState,
  getShownServerId,
  selfUser,
  subscribeAllServerChanges,
  subscribeServerChanges,
  type ServerState,
} from "./serverStore";

let selected: Conversation | null = null;
let syncedServer: number | null = null;
/** The server `selected` belongs to. */
let selectedServer: number | null = null;
/** Selections of the connected servers that are not shown. */
const remembered = new Map<number, Conversation | null>();
let watching = false;
const listeners = new Set<() => void>();

function setSelected(next: Conversation | null): void {
  if (next === selected) return;
  selected = next;
  for (const listener of listeners) listener();
}

function ownChannel(state: ServerState): Conversation | null {
  const self = selfUser(state);
  return self ? { kind: "channel", channelId: self.channelId } : null;
}

/** `conversation` if it still exists on the server, else the own channel. */
function validFor(state: ServerState, conversation: Conversation | null): Conversation | null {
  if (state.serverId === null) return null;
  if (conversation?.kind === "channel" && !state.channels.has(conversation.channelId)) {
    return ownChannel(state);
  }
  return conversation ?? ownChannel(state);
}

function reconcile(state: ServerState): void {
  if (state.serverId === null) syncedServer = null;
  setSelected(validFor(state, selected));
}

function onShown(state: ServerState): void {
  if (selectedServer !== null) remembered.set(selectedServer, selected);
  selectedServer = getShownServerId();
  syncedServer = state.serverId;
  const back = selectedServer === null ? null : (remembered.get(selectedServer) ?? null);
  if (selectedServer !== null) remembered.delete(selectedServer);
  setSelected(validFor(state, back));
}

function watch(): void {
  if (watching) return;
  watching = true;
  const initial = getServerState();
  syncedServer = initial.serverId;
  selectedServer = getShownServerId();
  reconcile(initial);
  // Plain store subscription would miss "synced" vs. "updated"; the change feed tells them apart.
  subscribeServerChanges((change, state) => {
    if (change.kind === "shown") {
      onShown(state);
    } else if (change.kind === "synced" && syncedServer === null) {
      syncedServer = state.serverId;
      setSelected(ownChannel(state));
    } else {
      reconcile(state);
    }
  });
  subscribeAllServerChanges((serverId, change, state) => {
    if (change.kind === "connection" && state.serverId === null) remembered.delete(serverId);
  });
}

function subscribe(listener: () => void): () => void {
  watch();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): Conversation | null {
  return selected;
}

/** Selects a conversation for the chat column. */
export function selectConversation(conversation: Conversation | null): void {
  setSelected(conversation);
}

export function getSelectedConversation(): Conversation | null {
  return selected;
}

/** The selected conversation and a setter. Stable API for B21/B22. */
export function useSelectedConversation(): {
  conversation: Conversation | null;
  select: (conversation: Conversation | null) => void;
} {
  const conversation = useSyncExternalStore(subscribe, getSnapshot);
  const select = useCallback((next: Conversation | null) => {
    setSelected(next);
  }, []);
  return { conversation, select };
}

/** True when `a` and `b` name the same conversation. */
export function sameConversation(a: Conversation | null, b: Conversation | null): boolean {
  if (a === null || b === null) return a === b;
  switch (a.kind) {
    case "channel":
      return b.kind === "channel" && b.channelId === a.channelId;
    case "private":
      return b.kind === "private" && b.certHash === a.certHash;
    case "privateSession":
      return b.kind === "privateSession" && b.session === a.session;
  }
}

/** Test helper. */
export function resetSelection(): void {
  selected = null;
  syncedServer = null;
  selectedServer = null;
  remembered.clear();
  watching = false;
  listeners.clear();
}
