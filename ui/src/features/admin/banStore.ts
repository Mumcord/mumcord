/**
 * The Server administration window (Epic 4, issue #254). One window per server is open at a time;
 * its ban list lives in memory only (FR-10.63), is read when the window opens and dropped when it
 * closes or the connection ends. Changes are collected here and saved together (FR-10.66): a save
 * first re-reads the server's list, and an entry someone else changed meanwhile becomes a conflict
 * the UI asks about before anything is sent.
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

export interface BanWindow {
  serverId: number;
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

export function openServerAdmin(serverId: number): void {
  set({
    serverId,
    entries: [],
    loading: true,
    error: null,
    changes: new Map(),
    revealed: new Set(),
    conflict: null,
  });
  void load();
}

/** Closing drops what the window held (FR-10.63). */
export function closeServerAdmin(): void {
  if (current) void getApi().banListClose(current.serverId);
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
