/**
 * Whisper and shout (issue #67): the saved targets, the own whisper (started from the UI or by a
 * target's key, reported through `whisperStateChanged`) and who whispers or shouts to the user
 * right now. Whispers always go through the voice server (FR-2.2), so incoming marks belong to it
 * and are dropped whenever that connection changes.
 */
import { useSyncExternalStore } from "react";
import {
  getApi,
  type IncomingWhisperKind,
  type MumcordApi,
  type Unsubscribe,
  type WhisperReceivers,
  type WhisperSetup,
  type WhisperState,
  type WhisperTarget,
} from "../../api";
import { getVoiceServerId, useShownServerId, useVoiceServerId } from "../channels/serverStore";
import { openSettingsSection } from "../settings/settingsOpener";

/** A target the person or channel menu asked to create; Settings opens its editor with it. */
export interface WhisperDraft {
  name: string;
  receivers: WhisperReceivers;
}

interface Snapshot {
  setup: WhisperSetup | null;
  own: WhisperState;
  incoming: ReadonlyMap<number, IncomingWhisperKind>;
  draft: WhisperDraft | null;
}

const IDLE: WhisperState = { state: "idle" };
const NO_MARKS: ReadonlyMap<number, IncomingWhisperKind> = new Map();

let snapshot: Snapshot = { setup: null, own: IDLE, incoming: NO_MARKS, draft: null };
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: Partial<Snapshot>): void {
  snapshot = { ...snapshot, ...next };
  for (const listener of listeners) listener();
}

function mark(session: number, kind: IncomingWhisperKind | null): void {
  if ((snapshot.incoming.get(session) ?? null) === kind) return;
  const incoming = new Map(snapshot.incoming);
  if (kind === null) incoming.delete(session);
  else incoming.set(session, kind);
  set({ incoming });
}

function isVoiceServer(serverId: number): boolean {
  return getVoiceServerId() === serverId;
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  snapshot = { setup: null, own: IDLE, incoming: NO_MARKS, draft: null };
  unsubscribers = [
    api.on("whisperStateChanged", ({ state }) => {
      set({ own: state });
    }),
    api.on("incomingWhisperChanged", ({ serverId, session, kind }) => {
      if (isVoiceServer(serverId)) mark(session, kind);
    }),
    api.on("userRemoved", ({ serverId, session }) => {
      if (isVoiceServer(serverId)) mark(session, null);
    }),
    api.on("serverSynced", ({ snapshot: synced }) => {
      if (isVoiceServer(synced.serverId)) set({ incoming: NO_MARKS });
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state !== "connected" && isVoiceServer(serverId)) set({ incoming: NO_MARKS });
    }),
    api.on("voiceServerChanged", () => {
      set({ incoming: NO_MARKS });
    }),
  ];
  api.whisperSetup().then(
    (setup) => {
      if (boundApi === api) set({ setup });
    },
    (error: unknown) => {
      console.error("Reading the whisper targets failed", error);
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

function read(): Snapshot {
  ensureBound();
  return snapshot;
}

/** The saved targets and whether their keys work; `null` until loaded. */
export function useWhisperSetup(): WhisperSetup | null {
  return useSyncExternalStore(subscribe, () => read().setup);
}

/** The own whisper: idle, active (with its target) or the reason the last start failed. */
export function useOwnWhisper(): WhisperState {
  return useSyncExternalStore(subscribe, () => read().own);
}

/** The target being whispered to right now, if any. */
export function useActiveWhisperTarget(): WhisperTarget | null {
  const own = useOwnWhisper();
  const setup = useWhisperSetup();
  if (own.state !== "active") return null;
  return setup?.targets.find((target) => target.id === own.targetId) ?? null;
}

/**
 * Whether `session` on the shown server whispers (`"whisper"`) or shouts (`"shout"`) to the user;
 * marks exist only for the voice server.
 */
export function useIncomingWhisper(session: number): IncomingWhisperKind | null {
  const shown = useShownServerId();
  const voice = useVoiceServerId();
  const kind = useSyncExternalStore(subscribe, () => read().incoming.get(session) ?? null);
  return voice !== null && voice === shown ? kind : null;
}

/** The pending "Whisper to…" draft, if a menu asked for one. */
export function useWhisperDraft(): WhisperDraft | null {
  return useSyncExternalStore(subscribe, () => read().draft);
}

/** Replaces the saved targets; rejects with the engine error (e.g. a key already in use). */
export async function saveWhisperTargets(targets: WhisperTarget[]): Promise<void> {
  ensureBound();
  const api = getApi();
  const setup = await api.whisperTargetsSave(targets);
  if (boundApi === api) set({ setup });
}

/** Starts whispering to a saved target until {@link stopWhisper}. */
export async function startWhisper(targetId: string): Promise<WhisperState> {
  ensureBound();
  const api = getApi();
  const own = await api.whisperStart(targetId);
  if (boundApi === api) set({ own });
  return own;
}

export async function stopWhisper(): Promise<void> {
  await getApi().whisperStop();
}

/** "Whisper to…" from a person or channel menu: opens Settings with a new target prefilled. */
export function createWhisperTarget(draft: WhisperDraft): void {
  ensureBound();
  set({ draft });
  openSettingsSection("voiceAudio");
}

/** The settings editor took over the draft. */
export function clearWhisperDraft(): void {
  if (snapshot.draft !== null) set({ draft: null });
}

/** Forgets a refusal once it was shown. */
export function dismissWhisperDenial(): void {
  if (snapshot.own.state === "denied") set({ own: IDLE });
}

/** A target's subtitle kind: people get a whisper, channels a shout (as Mumble names them). */
export function whisperKind(receivers: WhisperReceivers): IncomingWhisperKind {
  return receivers.kind === "people" ? "whisper" : "shout";
}

/** Test helper: forgets the binding and all state. */
export function resetWhisperStore(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  snapshot = { setup: null, own: IDLE, incoming: NO_MARKS, draft: null };
}
