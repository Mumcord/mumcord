/**
 * Whether the main area shows the "Message requests" view instead of the chat (issue #141). The
 * view belongs to the server it was opened on and to the conversation selected at that moment:
 * selecting any conversation (even the same channel again, which sets a new selection) or showing
 * another server returns to the chat.
 */
import { useSyncExternalStore } from "react";
import type { Conversation } from "../../api";
import { useShownServerId } from "../channels";
import { getSelectedConversation, useSelectedConversation } from "../channels/selection";

interface OpenView {
  serverId: number;
  over: Conversation | null;
  /** The request shown in the card; `null` shows the first one. */
  selectedId: number | null;
}

let view: OpenView | null = null;
const listeners = new Set<() => void>();

function set(next: OpenView | null): void {
  view = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): OpenView | null {
  return view;
}

export function openRequestsView(serverId: number): void {
  set({ serverId, over: getSelectedConversation(), selectedId: null });
}

export function selectRequest(id: number): void {
  if (view) set({ ...view, selectedId: id });
}

/** The open view on the shown server, or `null` when the chat shows. */
export function useRequestsView(): { serverId: number; selectedId: number | null } | null {
  const open = useSyncExternalStore(subscribe, getSnapshot);
  const shown = useShownServerId();
  const { conversation } = useSelectedConversation();
  if (open?.serverId !== shown || open.over !== conversation) return null;
  return { serverId: open.serverId, selectedId: open.selectedId };
}

/** Test helper. */
export function resetRequestsView(): void {
  view = null;
}
