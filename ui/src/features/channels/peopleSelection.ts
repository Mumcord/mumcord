/**
 * People picked in the channel tree with Ctrl/Cmd-click or Shift-click (Epic 4, issue #255), so
 * several of them can be dragged onto a channel together. The person menu moves the whole
 * selection when it is opened on one of the picked people, which is the keyboard way to the same
 * thing. A plain click clears the selection again.
 */
import { useSyncExternalStore } from "react";

let chosen: ReadonlySet<number> = new Set();
/** Where a Shift-click range starts; `null` until a person was picked. */
let anchor: number | null = null;
const listeners = new Set<() => void>();

function setChosen(next: ReadonlySet<number>): void {
  chosen = next;
  for (const listener of listeners) listener();
}

export function subscribePeopleSelection(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getPeopleSelection(): ReadonlySet<number> {
  return chosen;
}

export function isPersonSelected(session: number): boolean {
  return chosen.has(session);
}

/** The picked people as the tree's rows report them (one subscription per row). */
export function usePersonSelected(session: number): boolean {
  return useSyncExternalStore(subscribePeopleSelection, () => isPersonSelected(session));
}

/** Ctrl/Cmd-click: adds the person to the selection or takes them out again. */
export function togglePerson(session: number): void {
  const next = new Set(chosen);
  if (!next.delete(session)) next.add(session);
  anchor = session;
  setChosen(next);
}

/**
 * Shift-click: every person between the last picked one and this one, in the tree's row order.
 * Without an anchor it picks just this person.
 */
export function selectPeopleRange(order: readonly number[], session: number): void {
  const to = order.indexOf(session);
  if (to < 0) return;
  const from = anchor === null ? -1 : order.indexOf(anchor);
  if (from < 0) {
    anchor = session;
    setChosen(new Set([session]));
    return;
  }
  const [start, end] = from <= to ? [from, to] : [to, from];
  setChosen(new Set(order.slice(start, end + 1)));
}

export function clearPeopleSelection(): void {
  anchor = null;
  if (chosen.size > 0) setChosen(new Set());
}

/** Forgets people who left the server or were moved away by someone else. */
export function prunePeopleSelection(existing: ReadonlySet<number>): void {
  if ([...chosen].every((session) => existing.has(session))) return;
  setChosen(new Set([...chosen].filter((session) => existing.has(session))));
}

/** `session` plus everyone else picked: the people a move takes along. */
export function peopleToMove(session: number): number[] {
  return chosen.has(session) ? [...new Set([...chosen, session])] : [session];
}

/** The channel all of `sessions` sit in, when they share one; `null` otherwise (issue #255). */
export function sharedChannelOf(
  users: ReadonlyMap<number, { channelId: number }>,
  sessions: readonly number[],
): number | null {
  let shared: number | null = null;
  for (const session of sessions) {
    const channelId = users.get(session)?.channelId;
    if (channelId === undefined) return null;
    if (shared === null) shared = channelId;
    else if (shared !== channelId) return null;
  }
  return shared;
}

/** Drag data type of a person row (issues #172, #255); its value is the session list. */
export const PERSON_DRAG_TYPE = "application/x-mumcord-person";

export function dragPayload(sessions: readonly number[]): string {
  return sessions.join(",");
}

/** The sessions of a dropped person drag; empty for data that is not a session list. */
export function parseDragPayload(data: string): number[] {
  return data
    .split(",")
    .map(Number)
    .filter((session) => Number.isInteger(session) && session > 0);
}

/** Test helper. */
export function resetPeopleSelection(): void {
  chosen = new Set();
  anchor = null;
}
