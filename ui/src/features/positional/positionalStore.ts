/**
 * Who currently sends positional data (FR-1.72), from `positionalSendingChanged` and the
 * `senders` snapshot of `positional_status`. The engine expires the flag itself, so the store
 * only follows. Flags belong to one connection: dropped on every new sync, on disconnect and
 * when a person leaves. Positional audio always refers to the voice server (FR-2.2); without
 * one, to the shown server.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import {
  getShownServerId,
  getVoiceServerId,
  useShownServerId,
  useVoiceServerId,
} from "../channels/serverStore";

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

/** Whether an event of `serverId` is about the server positional audio refers to. */
function isPositionalServer(serverId: number): boolean {
  const target = getVoiceServerId() ?? getShownServerId();
  return target === null || target === serverId;
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  senders = EMPTY;
  unsubscribers = [
    api.on("positionalSendingChanged", ({ serverId, session, sending }) => {
      if (isPositionalServer(serverId)) change(session, sending);
    }),
    api.on("positionalStatusChanged", ({ status }) => {
      set(new Set(status.senders));
    }),
    api.on("serverSynced", ({ snapshot }) => {
      if (isPositionalServer(snapshot.serverId)) set(EMPTY);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "connected" && isPositionalServer(serverId)) set(EMPTY);
    }),
    api.on("userRemoved", ({ serverId, session }) => {
      if (isPositionalServer(serverId)) change(session, false);
    }),
    // The flags of the previous voice server name sessions of another server.
    api.on("voiceServerChanged", () => {
      set(EMPTY);
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
export function usePositionalSending(session: number, serverId?: number): boolean {
  const shownServer = useShownServerId();
  const shown = serverId ?? shownServer;
  const voice = useVoiceServerId();
  const sending = useSyncExternalStore(subscribe, () => {
    ensureBound();
    return senders.has(session);
  });
  // The session belongs to the shown (or given) server; the flags to the voice server.
  return sending && (voice === null || voice === shown);
}

/** Test helper: forgets the binding and all flags. */
export function resetPositionalStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  senders = EMPTY;
}
