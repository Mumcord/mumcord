/**
 * Who currently sends positional data (FR-1.72), from `positionalSendingChanged` and the
 * `senders` snapshot of `positional_status`. The engine expires the flag itself, so the store
 * only follows. Flags belong to one connection: dropped on every new sync, on disconnect and
 * when a person leaves.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";

type Senders = ReadonlySet<number>;

const EMPTY: Senders = new Set();

let senders: Senders = EMPTY;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: Senders): void {
  senders = next;
  for (const listener of listeners) listener();
}

function change(session: number, sending: boolean): void {
  if (senders.has(session) === sending) return;
  const next = new Set(senders);
  if (sending) next.add(session);
  else next.delete(session);
  set(next);
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  senders = EMPTY;
  unsubscribers = [
    api.on("positionalSendingChanged", ({ session, sending }) => {
      change(session, sending);
    }),
    api.on("positionalStatusChanged", ({ status }) => {
      set(new Set(status.senders));
    }),
    api.on("serverSynced", () => {
      set(EMPTY);
    }),
    api.on("connectionStateChanged", ({ state }) => {
      if (state.state !== "connected") set(EMPTY);
    }),
    api.on("userRemoved", ({ session }) => {
      change(session, false);
    }),
  ];
  // Catch up on people who were already sending before the first subscriber.
  api.positionalStatus().then(
    (status) => {
      if (boundApi !== api) return;
      const merged = new Set(senders);
      for (const session of status.senders) merged.add(session);
      if (merged.size !== senders.size) set(merged);
    },
    (error: unknown) => {
      console.error("Reading the positional status failed", error);
    },
  );
}

function ensureBound(): void {
  const api = getApi();
  if (api !== boundApi) bind(api);
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Whether a person (or the user, with the own session) currently sends positions. */
export function usePositionalSending(session: number): boolean {
  return useSyncExternalStore(subscribe, () => {
    ensureBound();
    return senders.has(session);
  });
}

/** Test helper: forgets the binding and all flags. */
export function resetPositionalStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  senders = EMPTY;
}
