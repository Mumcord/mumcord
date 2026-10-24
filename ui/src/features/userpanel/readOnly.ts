/**
 * Read-only state of the user panel: a fresh connect lands self-muted and deafened (reading along)
 * until the user explicitly joins a channel or undeafens. An automatic reconnect keeps the state
 * it had, and a deliberate deafen during a call never counts as read-only.
 */
import { useEffect, useState } from "react";
import {
  selfUser,
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
      return previous.synced
        ? previous
        : { readOnly: selfUser(state)?.selfDeaf ?? false, synced: true };
    case "userUpserted":
      return previous.readOnly && change.user.session === state.selfSession && !change.user.selfDeaf
        ? { ...previous, readOnly: false }
        : previous;
    default:
      return previous;
  }
}

export function useReadOnly(): boolean {
  const [state, setState] = useState(INITIAL_READ_ONLY);
  useEffect(
    () =>
      subscribeServerChanges((change, server) => {
        setState((previous) => nextReadOnly(previous, change, server));
      }),
    [],
  );
  return state.readOnly;
}
