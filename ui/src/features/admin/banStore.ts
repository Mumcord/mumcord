/**
 * The Server administration window and its Bans tab (Epic 4, issues #254 and #256).
 *
 * One window per server is open at a time; its ban list lives in memory only (FR-10.63), is read
 * when its tab opens and dropped when the window closes or the connection ends. Changes are
 * collected here and saved together (FR-10.66): a save first re-reads the server's list, and an
 * entry someone else changed meanwhile becomes a conflict the UI asks about before anything is sent.
 *
 * The window holds a tab, because FR-10.50 gives it a second one: the Registered users tab keeps
 * its own state in `registeredStore`, and the window drops both when it closes.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  toEngineError,
  type BanChangeInput,
  type BanEntry,
  type BanSaveResult,
  type EngineError,
} from "../../api";
import { closeRegistered, openRegistered } from "./registeredStore";

/** Which of the window's tabs is shown (FR-10.50). */
export type AdminTab = "bans" | "registered";

export interface BanWindow {
  serverId: number;
  /** The shown tab; the other one's list is read only when it is opened (FR-10.63). */
  tab: AdminTab;
  entries: BanEntry[];
  /** Reading the server's list. */
  loading: boolean;
  /** Last read or save failure, so the window can show it and retry. */
  error: EngineError | null;
  /** Changes made here that are not saved yet, keyed by the entry. */
  changes: Map<string, BanChangeInput>;
  /** Rows whose address the user revealed (masked again when the window closes, FR-10.51). */
  revealed: Set<string>;
  /** The conflict a save found, if any (FR-10.66). */
  conflict: { key: string; fresh: BanEntry[] } | null;
  /**
   * An add-ban form the Registered users tab asked for, with the account's name and certificate
   * written in (issue #256, FR-10.65). The address is still missing there, which is why the form
   * opens instead of saving anything: a ban entry needs the address the server reports.
   */
  prefill: { name: string; hash: string | null } | null;
}

let current: BanWindow | null = null;
const listeners = new Set<() => void>();

function set(next: BanWindow | null): void {
  current = next;
  for (const listener of listeners) listener();
}

function update(patch: Partial<BanWindow>): void {
  if (!current) return;
  set({ ...current, ...patch });
}

/** Tests start from an empty window. */
export function resetServerAdmin(): void {
  closeRegistered();
  set(null);
}

export function useServerAdmin(): BanWindow | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}

/** The open window outside React: a command must not drop the list an open window still uses. */
export function currentBanWindow(): BanWindow | null {
  return current;
}

/** Opens the window on one of its tabs and reads what that tab shows. */
export function openServerAdmin(serverId: number, tab: AdminTab = "bans"): void {
  set({
    serverId,
    tab,
    entries: [],
    loading: tab === "bans",
    error: null,
    changes: new Map(),
    revealed: new Set(),
    conflict: null,
    prefill: null,
  });
  // A window opened on the other tab must not wait for a list nobody is looking at (FR-10.63).
  if (tab === "bans") void load();
  else openRegistered(serverId);
}

/**
 * Moves to the other tab and reads its list now. The ban list is only read when its tab is shown
 * and it is still empty, so Refresh stays the one way to re-read it on purpose (FR-10.44).
 */
export function selectTab(tab: AdminTab): void {
  if (!current || current.tab === tab) return;
  set({ ...current, tab });
  if (tab === "registered") openRegistered(current.serverId);
  else if (current.entries.length === 0) void load();
}

/** Closing drops what either tab held (FR-10.63). */
export function closeServerAdmin(): void {
  if (current) void getApi().banListClose(current.serverId);
  closeRegistered();
  set(null);
}

/** Reads the list again: "Refresh", the conflict's "Reload" and the failed read's "Try again". */
export async function load(): Promise<void> {
  if (!current) return;
  const api = getApi();
  update({ loading: true, error: null });
  try {
    const entries = await api.banListOpen(current.serverId);
    update({ entries });
  } catch (error) {
    update({ error: toEngineError(error) });
  } finally {
    update({ loading: false });
  }
}

/**
 * Opens the Bans tab with the add form waiting for one account's ban (issue #256). The form is
 * what asks for the address, which the other tab does not have.
 */
export function requestBanCertificate(name: string, hash: string | null): void {
  if (!current) return;
  const serverId = current.serverId;
  set({ ...current, tab: "bans", prefill: { name, hash } });
  if (current.entries.length === 0) void load();
  openRegistered(serverId);
}

/** The form opened from the prefill: it is shown once. */
export function clearPrefill(): void {
  if (current) update({ prefill: null });
}

/** Stages one change; the window saves all of them together. */
export function stageChange(change: BanChangeInput): void {
  if (!current) return;
  const changes = new Map(current.changes);
  changes.set(changeKey(change), change);
  update({ changes });
}

export function discardChanges(): void {
  if (current) update({ changes: new Map(), conflict: null });
}

/** The conflict dialog's "Back": the changes stay, nothing is sent (FR-10.43). */
export function clearConflict(): void {
  if (current) update({ conflict: null });
}

export function pendingFor(row: BanWindow, entry: BanEntry): BanChangeInput | undefined {
  return row.changes.get(entry.key);
}

/** A pending change for an entry the user has not loaded yet (an added row). */
export function pendingAdd(row: BanWindow, change: BanChangeInput): BanChangeInput | undefined {
  return row.changes.get(changeKey(change));
}

export function reveal(key: string, show: boolean): void {
  if (!current) return;
  const revealed = new Set(current.revealed);
  if (show) revealed.add(key);
  else revealed.delete(key);
  update({ revealed });
}

/** Saves everything collected here (FR-10.66). `force` is the UI's "Save mine anyway". */
export async function saveChanges(force: boolean): Promise<BanSaveResult | null> {
  if (!current) return null;
  const api = getApi();
  const changes = [...current.changes.values()];
  if (changes.length === 0) return null;
  try {
    const outcome = await api.banListSave(current.serverId, changes, force);
    if (outcome.kind === "conflict") {
      update({ conflict: outcome, error: null });
      return outcome;
    }
    update({ entries: outcome.entries, changes: new Map(), conflict: null, error: null });
    return outcome;
  } catch (error) {
    update({ error: toEngineError(error) });
    return null;
  }
}

export function changeKey(change: BanChangeInput): string {
  // An added entry has no key from the server yet, so the window keys it by what it typed.
  return change.kind === "add"
    ? `${change.address}/${String(change.mask)}|${change.hash ?? ""}`
    : change.key;
}
