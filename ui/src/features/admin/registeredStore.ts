/**
 * The Registered users tab of the Server administration window (Epic 4, issue #256).
 *
 * One account at a time is changed, and each change is one message for that account — unlike the
 * ban list, where the whole list goes out together (issue: "changes are sent per user when
 * saving"). The list lives in memory for the connection only: it is read when the tab opens and
 * dropped when the window closes or the connection ends (FR-10.63).
 *
 * The server does not confirm a change in words. It answers a query with its list, and that list
 * is the only account of what stands (measured against the local test server: a rename onto a name
 * another account holds is taken and dropped without a trace). So every save reads the list back
 * and reports from what the server says, never from what was typed.
 */
import { useSyncExternalStore } from "react";
import { getApi, toEngineError, type EngineError, type RegisteredUserEntry } from "../../api";

/** What the tab will send for one account when the user saves. */
export type RegisteredChange = { kind: "rename"; name: string } | { kind: "unregister" };

export interface RegisteredWindow {
  serverId: number;
  entries: RegisteredUserEntry[];
  /** Reading the server's list. */
  loading: boolean;
  /** Last read or save failure, so the tab can show it and retry. */
  error: EngineError | null;
  /** The account the detail panel shows. */
  selected: number | null;
  /** The name typed in the panel, `null` while the field holds the server's name. */
  draft: string | null;
  /** The accounts marked for unregistering, which the confirmation put here. */
  removals: number[];
  /** Sending the changes. */
  busy: boolean;
  /** Groups of the selected account, `null` while they are not read or known. */
  groups: string[] | null;
  /**
   * An account whose change the server did not take. It does not say why; the usual cause is a
   * name another account already holds (issue #256).
   */
  kept: number | null;
  /** What the last save did, for the line the tab says back. */
  saved: { renamed: number; unregistered: number } | null;
}

let current: RegisteredWindow | null = null;
const listeners = new Set<() => void>();

function set(next: RegisteredWindow | null): void {
  current = next;
  for (const listener of listeners) listener();
}

function update(patch: Partial<RegisteredWindow>): void {
  if (!current) return;
  set({ ...current, ...patch });
}

/** Tests start from an empty tab. */
export function resetRegistered(): void {
  set(null);
}

export function useRegistered(): RegisteredWindow | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}

/** Opens the tab and reads the server's list (FR-10.63: fresh every time). */
export function openRegistered(serverId: number): void {
  set({
    serverId,
    entries: [],
    loading: true,
    error: null,
    selected: null,
    draft: null,
    removals: [],
    busy: false,
    groups: null,
    kept: null,
    saved: null,
  });
  void load();
}

/** Closing drops what the tab held (FR-10.63). */
export function closeRegistered(): void {
  if (current) void getApi().registeredUsersClose(current.serverId);
  set(null);
}

/**
 * The window of `serverId`, when it is still the open one. Every await ends with this check, so a
 * late answer from a connection the user has left cannot write into another server's window.
 */
function alive(serverId: number): RegisteredWindow | null {
  return current && current.serverId === serverId ? current : null;
}

/** Reads the list again: "Refresh", the failed read's "Try again" and every save. */
export async function load(): Promise<void> {
  if (!current) return;
  const api = getApi();
  const serverId = current.serverId;
  update({ loading: true, error: null });
  try {
    const entries = await api.registeredUsersOpen(serverId);
    const open = alive(serverId);
    if (!open) return;
    // A row the server no longer has closes its panel.
    update({
      entries,
      selected:
        open.selected === null || entries.some((e) => e.userId === open.selected)
          ? open.selected
          : null,
    });
  } catch (error) {
    if (alive(serverId)) update({ error: toEngineError(error) });
  } finally {
    if (alive(serverId)) update({ loading: false });
  }
}

/**
 * Opens the detail panel of one account. The name field starts on the server's name, and the
 * groups are read from the server: it looks them up where the account was last seen, or at the
 * root when it does not say. A server that will not show its groups leaves the row out.
 */
export function select(userId: number | null): void {
  if (!current) return;
  set({
    ...current,
    selected: userId,
    draft: null,
    kept: null,
    saved: null,
    groups: null,
  });
  loadGroups(userId);
}

/**
 * Reads the groups of one account without touching what the panel says about the last change: a
 * save re-reads them, and its summary must stay on screen (issue #256).
 */
function loadGroups(userId: number | null): void {
  if (!current || userId === null) return;
  const api = getApi();
  const serverId = current.serverId;
  void api
    .registeredUserGroups(serverId, userId)
    .then((groups) => {
      if (alive(serverId)?.selected !== userId) return;
      update({ groups });
    })
    .catch(() => {
      // The server would not say; the panel then shows no groups. The engine logs the reason.
      if (alive(serverId)?.selected === userId) update({ groups: [] });
    });
}

/** The name typed in the panel; the change goes out when the user saves. */
export function setDraft(name: string): void {
  if (!current) return;
  update({ draft: name, kept: null, saved: null });
}

/** The name the panel shows for an account: the typed one while it differs, else the server's. */
export function draftOf(window: RegisteredWindow, entry: RegisteredUserEntry): string {
  return window.selected === entry.userId && window.draft !== null ? window.draft : entry.name;
}

/** The rename waiting to go out, or `null` when the field still says what the server does. */
export function pendingRename(window: RegisteredWindow): RegisteredUserEntry | null {
  if (window.selected === null || window.draft === null) return null;
  const entry = window.entries.find((candidate) => candidate.userId === window.selected);
  if (!entry || entry.name === window.draft.trim() || window.draft.trim() === "") return null;
  return entry;
}

/** Marks one account for unregistering, after its confirmation (FR-10.52). */
export function stageUnregister(userId: number): void {
  if (!current) return;
  if (current.removals.includes(userId)) return;
  update({ removals: [...current.removals, userId], kept: null, saved: null });
}

export function undoUnregister(userId: number): void {
  if (!current) return;
  update({ removals: current.removals.filter((id) => id !== userId) });
}

/** What the tab has queued but not sent yet (FR-10.63: nothing of this survives the window). */
export function pendingCount(window: RegisteredWindow): number {
  return window.removals.length + (pendingRename(window) ? 1 : 0);
}

export function dropChanges(): void {
  if (!current) return;
  update({ removals: [], draft: null, kept: null, saved: null });
}

/**
 * Sends the queued changes, one message per account, and reads the list back. `force` is not a
 * thing here: the server's list always wins, and a change it did not take is reported as such.
 */
export async function saveChanges(): Promise<void> {
  const window = current;
  if (!window || window.busy) return;
  const api = getApi();
  const serverId = window.serverId;
  const rename = pendingRename(window);
  const removals = [...window.removals];
  if (!rename && removals.length === 0) return;
  set({ ...window, busy: true, error: null, kept: null, saved: null });
  let renamed = 0;
  let unregistered = 0;
  let kept: number | null = null;
  let entries = window.entries;
  try {
    for (const userId of removals) {
      const outcome = await api.registeredUserUnregister(serverId, userId);
      entries = outcome.entries;
      if (outcome.kind === "saved") unregistered += 1;
      else kept = userId;
    }
    if (rename) {
      const wanted = (window.draft ?? "").trim();
      const outcome = await api.registeredUserRename(serverId, rename.userId, wanted);
      entries = outcome.entries;
      if (outcome.kind === "saved") renamed += 1;
      else kept = rename.userId;
    }
    if (!alive(serverId)) return;
    update({
      entries,
      busy: false,
      // A change the server took leaves the queue; one it did not take stays where the user can
      // see it and try again.
      removals: kept !== null && removals.includes(kept) ? [kept] : [],
      draft: kept !== null && rename && kept === rename.userId ? window.draft : null,
      kept,
      saved: { renamed, unregistered },
      groups: null,
    });
    if (alive(serverId)?.selected !== null) loadGroups(alive(serverId)?.selected ?? null);
  } catch (error) {
    if (alive(serverId)) update({ busy: false, error: toEngineError(error) });
  }
}

/** Registers an online guest under their current name (FR-10.20, AC-10.15). */
export async function registerPerson(
  serverId: number,
  session: number,
): Promise<{ ok: true } | { ok: false; error: EngineError }> {
  try {
    await getApi().personRegister(serverId, session);
    // The list may already hold the new account; read it again when the tab is open.
    if (alive(serverId)) void load();
    return { ok: true };
  } catch (error) {
    return { ok: false, error: toEngineError(error) };
  }
}
