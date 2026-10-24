/**
 * The conversation the chat column shows ("selected conversation"). Shared by the channel tree
 * (single click), the member list (send private message), chat (B21) and follow/PMs (B22).
 *
 * - After each `serverSynced` it starts at the own channel; on disconnect it becomes `null`.
 * - If the selected channel is removed it falls back to the own channel.
 */
import { useCallback, useSyncExternalStore } from "react";
import type { Conversation } from "../../api";
import { getServerState, selfUser, subscribeServerChanges, type ServerState } from "./serverStore";

let selected: Conversation | null = null;
let syncedServer: number | null = null;
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

function reconcile(state: ServerState): void {
  if (state.serverId === null) {
    syncedServer = null;
    setSelected(null);
    return;
  }
  if (selected?.kind === "channel" && !state.channels.has(selected.channelId)) {
    setSelected(ownChannel(state));
  } else if (selected === null) {
    setSelected(ownChannel(state));
  }
}

function watch(): void {
  if (watching) return;
  watching = true;
  const initial = getServerState();
  syncedServer = initial.serverId;
  reconcile(initial);
  // Plain store subscription would miss "synced" vs. "updated"; the change feed tells them apart.
  subscribeServerChanges((change, state) => {
    if (change.kind === "synced" && syncedServer === null) {
      syncedServer = state.serverId;
      setSelected(ownChannel(state));
    } else {
      reconcile(state);
    }
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
  watching = false;
  listeners.clear();
}
