/**
 * The channel-password dialog (FR-2.78). The engine reports a join refused for a missing Enter
 * permission; the typed password is sent as an access token and the join retried. The server's
 * answer comes back as another `channelPasswordPrompt` step: `wrong` keeps the dialog open,
 * `ended` closes it. Only one prompt is shown; a newer refusal replaces it.
 */
import { useSyncExternalStore } from "react";
import { getApi, toEngineError, type MumcordApi, type Unsubscribe } from "../../api";

export interface ChannelPasswordPrompt {
  serverId: number;
  channelId: number;
  /** Waiting for the server's answer to a typed password. */
  busy: boolean;
  /** The server refused the last typed password. */
  wrong: boolean;
  /** Sending the password failed (e.g. secure storage unavailable). */
  failed: boolean;
}

let prompt: ChannelPasswordPrompt | null = null;
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function set(next: ChannelPasswordPrompt | null): void {
  prompt = next;
  for (const listener of listeners) listener();
}

function isShown(serverId: number, channelId: number): boolean {
  return prompt?.serverId === serverId && prompt.channelId === channelId;
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  prompt = null;
  unsubscribers = [
    api.on("channelPasswordPrompt", ({ serverId, channelId, step }) => {
      if (step === "ended") {
        if (isShown(serverId, channelId)) set(null);
        return;
      }
      set({ serverId, channelId, busy: false, wrong: step === "wrong", failed: false });
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (prompt?.serverId === serverId && state.state !== "connected") set(null);
    }),
  ];
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

function getPrompt(): ChannelPasswordPrompt | null {
  return prompt;
}

export function useChannelPasswordPrompt(): ChannelPasswordPrompt | null {
  return useSyncExternalStore(subscribe, getPrompt);
}

/** Sends the typed password; the server's answer arrives as the next prompt step. */
export async function submitChannelPassword(password: string): Promise<void> {
  const current = prompt;
  if (!current || current.busy) return;
  set({ ...current, busy: true, wrong: false, failed: false });
  try {
    await getApi().channelJoinWithPassword(current.serverId, current.channelId, password);
  } catch (cause: unknown) {
    console.error("Sending the channel password failed", toEngineError(cause).code);
    if (isShown(current.serverId, current.channelId)) {
      set({ ...current, busy: false, wrong: false, failed: true });
    }
  }
}

export function cancelChannelPassword(): void {
  set(null);
}
