/**
 * The kick and ban dialogs of a person (Epic 4, issue #253). One dialog is open at a time, and what
 * it holds lives in memory for the connection only (FR-10.63): it is dropped when the dialog closes
 * or the connection ends. The store keeps what the dialog reads from the engine plus the form, so
 * "Kick only instead" keeps what the user already typed.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  toEngineError,
  type BanEntry,
  type BanInput,
  type EngineError,
  type PersonTarget,
} from "../../api";

/** Kick and ban (#253), and the reset confirmations of #257. */
export type PersonActionMode = "kick" | "ban" | "resetComment" | "resetAvatar";

/** The form as the ban dialog shows it (FR-10.12); the kick dialog uses only its reason. */
export interface PersonDraft {
  reason: string;
  /** Seconds; 0 is permanent. Ignored on the quick path, which uses the server's own length. */
  duration: number;
  /** Subnet length in the family of the address: 32 for a single IPv4, 128 for a single IPv6. */
  mask: number;
  byCert: boolean;
  byIp: boolean;
  removeNow: boolean;
}

export interface PersonAction {
  serverId: number;
  session: number;
  /** Name and channel as the person menu had them (FR-10.57). */
  name: string;
  channel: string;
  mode: PersonActionMode;
  /** Address, certificate, connected time, whether the ban list can be read, and the rights. */
  target: PersonTarget | null;
  draft: PersonDraft;
  loading: boolean;
  busy: boolean;
  /** A refusal, shown inside the open dialog (FR-10.59). */
  error: EngineError | null;
  /** The entry the server changed meanwhile, which the dialog asks about (FR-10.66). */
  conflict: { key: string; fresh: BanEntry[] } | null;
}

let current: PersonAction | null = null;
const listeners = new Set<() => void>();

function set(next: PersonAction | null): void {
  current = next;
  for (const listener of listeners) listener();
}

function update(patch: Partial<PersonAction>): void {
  if (!current) return;
  set({ ...current, ...patch });
}

/** Tests start with no dialog open. */
export function resetPersonAction(): void {
  set(null);
}

export function usePersonAction(): PersonAction | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}

/**
 * Opens the dialog. The defaults follow the mockup and the issue: the reason first, 1 hour,
 * certificate when the person has one, address when the server can show it, and the person removed
 * right away. A kick needs no reading; the ban dialog asks the server for its facts.
 */
export function openPersonAction(
  serverId: number,
  session: number,
  name: string,
  channel: string,
  mode: PersonActionMode,
): void {
  set({
    serverId,
    session,
    name,
    channel,
    mode,
    target: null,
    draft: { reason: "", duration: 3600, mask: 32, byCert: true, byIp: true, removeNow: true },
    loading: mode === "ban",
    busy: false,
    error: null,
    conflict: null,
  });
  if (mode === "ban") void loadTarget();
}

/** Closing drops what the dialog held (FR-10.63). */
export function closePersonAction(): void {
  if (current) void getApi().personActionClose(current.serverId);
  set(null);
}

/** "Kick only instead" (FR-10.12): the same dialog without the duration and the targets. */
export function switchMode(mode: PersonActionMode): void {
  if (!current) return;
  const action = current;
  update({ mode, error: null, conflict: null });
  if (mode === "ban" && !action.target) void loadTarget();
}

export function setDraft(patch: Partial<PersonDraft>): void {
  if (!current) return;
  set({ ...current, draft: { ...current.draft, ...patch } });
}

/** Reads what the ban dialog needs: address, certificate, and whether the list can be read. */
export async function loadTarget(): Promise<void> {
  if (!current) return;
  const action = current;
  const api = getApi();
  update({ loading: true, error: null });
  try {
    const target = await api.personTarget(action.serverId, action.session);
    const v6 = target.address !== null && target.address.includes(":");
    // A target the server cannot match stays off with its reason shown (FR-10.14).
    setDraft({
      mask: v6 ? 128 : 32,
      byCert: target.certHash !== null,
      byIp: target.address !== null,
    });
    update({ target });
  } catch (error) {
    update({ error: toEngineError(error) });
  } finally {
    update({ loading: false });
  }
}

/** The change as the engine takes it (FR-10.12, FR-10.13). */
export function banInput(action: PersonAction): BanInput {
  return {
    reason: action.draft.reason.trim() === "" ? null : action.draft.reason.trim(),
    duration: action.target?.banListReadable ? action.draft.duration : 0,
    byCert: action.target === null || action.target.certHash === null ? false : action.draft.byCert,
    byIp: action.target === null || action.target.address === null ? false : action.draft.byIp,
    mask: action.draft.mask,
    removeNow: action.draft.removeNow,
    force: false,
    quick: action.target !== null && !action.target.banListReadable,
  };
}

export async function sendKick(reason: string | null): Promise<void> {
  if (!current) return;
  const action = current;
  const api = getApi();
  update({ busy: true, error: null });
  try {
    await api.personKick(action.serverId, action.session, reason);
    closePersonAction();
  } catch (error) {
    update({ error: toEngineError(error), busy: false });
  }
}

export async function sendBan(input: BanInput): Promise<void> {
  if (!current) return;
  const action = current;
  const api = getApi();
  update({ busy: true, error: null, conflict: null });
  try {
    const result = await api.personBan(action.serverId, action.session, input);
    if (result.kind === "conflict") {
      // Nothing was sent; the dialog asks before the server's entry is overwritten (FR-10.66).
      update({ conflict: { key: result.key, fresh: result.fresh }, busy: false });
      return;
    }
    closePersonAction();
  } catch (error) {
    update({ error: toEngineError(error), busy: false });
  }
}

/** The conflict's "Save mine anyway": the same change on top of the server's fresh list. */
export function saveAnyway(input: BanInput): void {
  void sendBan({ ...input, force: true });
}
