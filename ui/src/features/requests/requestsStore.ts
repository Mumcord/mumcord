/**
 * Chat and message requests per server and the accepted conversations (issue #141), as the engine
 * reports them. Requests live in memory only: they are loaded when a server syncs, follow every
 * `pmRequestsChanged` and are dropped on disconnect. One store for the app; it binds to the active
 * engine client and re-binds when tests swap it.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type AcceptedConversation,
  type MumcordApi,
  type PmRequests,
  type Unsubscribe,
} from "../../api";
import { useServers } from "../servers/store";
import { ownCertHash } from "./ownIdentity";

const NO_REQUESTS: PmRequests = { incoming: [], sent: [] };

let byServer: ReadonlyMap<number, PmRequests> = new Map();
let accepted: readonly AcceptedConversation[] = [];
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
let generation = 0;
/** Bumped by every change of a server's requests, so a load started earlier cannot overwrite it. */
const versions = new Map<number, number>();
let acceptedChanged = false;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

function setRequests(serverId: number, requests: PmRequests | null): void {
  versions.set(serverId, (versions.get(serverId) ?? 0) + 1);
  const next = new Map(byServer);
  if (requests === null) next.delete(serverId);
  else next.set(serverId, requests);
  byServer = next;
  notify();
}

function load(api: MumcordApi, serverId: number): void {
  const mine = generation;
  const version = versions.get(serverId) ?? 0;
  api.pmRequests(serverId).then(
    (requests) => {
      if (generation === mine && (versions.get(serverId) ?? 0) === version) {
        setRequests(serverId, requests);
      }
    },
    (error: unknown) => {
      console.error("Loading the message requests failed", error);
    },
  );
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  byServer = new Map();
  accepted = [];
  versions.clear();
  acceptedChanged = false;
  const mine = ++generation;
  unsubscribers = [
    api.on("pmRequestsChanged", ({ serverId, requests }) => {
      setRequests(serverId, requests);
    }),
    api.on("serverSynced", ({ snapshot }) => {
      load(api, snapshot.serverId);
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state === "disconnected" && byServer.has(serverId)) setRequests(serverId, null);
    }),
    api.on("acceptedConversationsChanged", ({ conversations }) => {
      acceptedChanged = true;
      accepted = conversations;
      notify();
    }),
  ];
  api.connectionsList().then(
    (connections) => {
      if (generation !== mine) return;
      for (const { serverId, state } of connections) {
        if (state.state === "connected") load(api, serverId);
      }
    },
    (error: unknown) => {
      console.error("Listing the connections for message requests failed", error);
    },
  );
  api.acceptedConversations().then(
    (list) => {
      if (generation !== mine || acceptedChanged) return;
      accepted = list;
      notify();
    },
    (error: unknown) => {
      console.error("Loading the accepted conversations failed", error);
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

function getRequestsSnapshot(): ReadonlyMap<number, PmRequests> {
  ensureBound();
  return byServer;
}

function getAcceptedSnapshot(): readonly AcceptedConversation[] {
  ensureBound();
  return accepted;
}

/** Held incoming and sent requests of one server; empty while it is not connected. */
export function usePmRequests(serverId: number | null): PmRequests {
  const all = useSyncExternalStore(subscribe, getRequestsSnapshot);
  return (serverId === null ? undefined : all.get(serverId)) ?? NO_REQUESTS;
}

/** Held incoming and sent requests of every connected server, by server id. */
export function useAllPmRequests(): ReadonlyMap<number, PmRequests> {
  return useSyncExternalStore(subscribe, getRequestsSnapshot);
}

/** Conversations the user accepted, for every own identity. */
export function useAcceptedConversations(): readonly AcceptedConversation[] {
  return useSyncExternalStore(subscribe, getAcceptedSnapshot);
}

export function acceptRequest(serverId: number, id: number): Promise<null> {
  return getApi().pmRequestAccept(serverId, id);
}

export function declineRequest(serverId: number, id: number): Promise<null> {
  return getApi().pmRequestDecline(serverId, id);
}

/** Sends a chat request; an empty note is sent as none. */
export function sendChatRequest(serverId: number, session: number, note: string): Promise<null> {
  const trimmed = note.trim();
  return getApi().chatRequestSend(serverId, session, trimmed === "" ? null : trimmed);
}

/** The accepted conversation between the own identity on `serverId` and `certHash`, if any. */
export function useAcceptedConsent(
  serverId: number | null,
  certHash: string | null,
): AcceptedConversation | undefined {
  const accepted = useAcceptedConversations();
  const { servers, identities } = useServers();
  if (serverId === null || certHash === null) return undefined;
  const own = ownCertHash(servers, identities, serverId);
  return accepted.find((c) => c.ownCertHash === own && c.certHash === certHash);
}

export function revokeAccepted(conversation: AcceptedConversation): Promise<null> {
  return getApi().acceptedConversationRevoke(conversation.ownCertHash, conversation.certHash);
}

/** Test helper. */
export function resetRequestsStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  generation += 1;
  byServer = new Map();
  accepted = [];
  versions.clear();
}
