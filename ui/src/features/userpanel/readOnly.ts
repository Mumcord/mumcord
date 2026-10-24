/**
 * Read-only state of the user panel: a fresh connect lands self-muted and deafened (reading along)
 * until the user explicitly joins a channel or undeafens. An automatic reconnect keeps the state
 * it had, and a deliberate deafen during a call never counts as read-only. A join restores the
 * user's last own mute/deafen choice (QA.7), which may itself be deafened, so the join is
 * signalled explicitly instead of being inferred from the deafen flag. The first sync takes the
 * engine's own read-only flag from the snapshot, which is false after a `mumble://` link join.
 */
import { useEffect, useState } from "react";
import { getApi } from "../../api";
import {
  subscribeServerChanges,
  type ServerChange,
  type ServerState,
} from "../channels/serverStore";

export interface ReadOnlyState {
  readOnly: boolean;
  /** The current connection has synced before, so a further sync is a reconnect. */
  synced: boolean;
}

export const INITIAL_READ_ONLY: ReadOnlyState = { readOnly: false, synced: false };

export function nextReadOnly(
  previous: ReadOnlyState,
  change: ServerChange,
  state: ServerState,
): ReadOnlyState {
  switch (change.kind) {
    case "connection":
      return state.connection.state === "disconnected" || state.connection.state === "connecting"
        ? INITIAL_READ_ONLY
        : previous;
    case "synced":
      return previous.synced ? previous : { readOnly: state.readOnly, synced: true };
    case "userUpserted":
      return previous.readOnly && change.user.session === state.selfSession && !change.user.selfDeaf
        ? { ...previous, readOnly: false }
        : previous;
    default:
      return previous;
  }
}

const joinListeners = new Set<() => void>();

/** Explicit join of a channel (double-click, Join button): leaves the read-only state. */
export async function joinChannel(channelId: number): Promise<void> {
  await getApi().channelJoin(channelId);
  for (const listener of joinListeners) listener();
}

export function useReadOnly(): boolean {
  const [state, setState] = useState(INITIAL_READ_ONLY);
  useEffect(() => {
    const unsubscribe = subscribeServerChanges((change, server) => {
      setState((previous) => nextReadOnly(previous, change, server));
    });
    const joined = () => {
      setState((previous) => ({ ...previous, readOnly: false }));
    };
    joinListeners.add(joined);
    return () => {
      unsubscribe();
      joinListeners.delete(joined);
    };
  }, []);
  return state.readOnly;
}
