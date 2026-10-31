/**
 * State of the notification centre (#143): the collected entries, which requests were read or
 * dismissed, and whether the panel is open. In memory only, so a full quit clears it (FR-3.107).
 */
import { useSyncExternalStore } from "react";
import type { Conversation } from "../../api";
import { conversationKey } from "../chat";
import { prune, type CentreState, type StoredItem } from "./centreModel";

let state: CentreState = { items: [], readRequests: new Set(), dismissedRequests: new Set() };
let open = false;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setItems(items: readonly StoredItem[]): void {
  if (items === state.items) return;
  state = { ...state, items };
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useCentreState(): CentreState {
  return useSyncExternalStore(subscribe, () => state);
}

export function getCentreState(): CentreState {
  return state;
}

/** Adds an entry, replacing one with the same id; old mentions and replies are pruned. */
export function addItem(item: StoredItem, now = Date.now()): void {
  setItems(prune([...state.items.filter((i) => i.id !== item.id), item], now));
}

/** Drops mentions and replies past their time, e.g. when the panel opens after days. */
export function pruneItems(now = Date.now()): void {
  setItems(prune(state.items, now));
}

export function hasItem(id: string): boolean {
  return state.items.some((item) => item.id === id);
}

/** Removes stored entries, e.g. an invite whose party ended. */
export function removeItems(match: (item: StoredItem) => boolean): void {
  if (state.items.some(match)) setItems(state.items.filter((item) => !match(item)));
}

function markItems(match: (item: StoredItem) => boolean): void {
  if (!state.items.some((item) => !item.read && match(item))) return;
  setItems(state.items.map((item) => (!item.read && match(item) ? { ...item, read: true } : item)));
}

/** Marks one entry read; `id` may also be a request's. */
export function markRead(id: string): void {
  if (id.startsWith("request/")) {
    if (state.readRequests.has(id)) return;
    state = { ...state, readRequests: new Set(state.readRequests).add(id) };
    notify();
    return;
  }
  markItems((item) => item.id === id);
}

/** "Mark all as read": every stored entry and the given requests (the ones shown). */
export function markAllRead(requestIds: readonly string[]): void {
  const readRequests = new Set([...state.readRequests, ...requestIds]);
  state = {
    ...state,
    items: state.items.map((item) => (item.read ? item : { ...item, read: true })),
    readRequests,
  };
  notify();
}

/**
 * The chat was seen (on screen, or marked read in the private messages overview): its entries
 * are read too, so the centre and the overview agree (FR-3.110).
 */
export function markConversationRead(serverId: number, conversation: Conversation): void {
  const key = conversationKey(conversation);
  markItems(
    (item) =>
      item.serverId === serverId &&
      "conversation" in item &&
      conversationKey(item.conversation) === key,
  );
}

/** Removes an entry from the centre; a request stays in the requests view. */
export function dismissItem(id: string): void {
  if (id.startsWith("request/")) {
    state = { ...state, dismissedRequests: new Set(state.dismissedRequests).add(id) };
    notify();
    return;
  }
  removeItems((item) => item.id === id);
}

export function useCentreOpen(): boolean {
  return useSyncExternalStore(subscribe, () => open);
}

export function setCentreOpen(next: boolean): void {
  if (open === next) return;
  open = next;
  notify();
}

export function toggleCentre(): void {
  setCentreOpen(!open);
}

/** Test helper. */
export function resetCentre(): void {
  state = { items: [], readRequests: new Set(), dismissedRequests: new Set() };
  open = false;
}
