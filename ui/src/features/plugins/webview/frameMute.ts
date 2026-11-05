/**
 * Sound from plugin frames (FR-9.43). The user mutes one plugin's frames with the frame's mute
 * button, and all frames are muted while the user is deafened. Both go through Rust
 * (`plugin_frames_mute` → `plugin://frame-mute`) so every window, including a popped-out stage,
 * hears them. Rust keeps the current state: a window reads it once when it starts listening, so a
 * mute or deafen from before it opened still reaches its frames, and each frame sends its page
 * the result again after it opens.
 */
import { useEffect, useSyncExternalStore } from "react";
import { getApi, toEngineError, type MumcordApi, type Unsubscribe } from "../../../api";
import { selfUser, useServerStateOf, useVoiceServerId } from "../../channels/serverStore";

interface MuteState {
  /** Plugins the user muted with the frame's button. */
  readonly plugins: ReadonlySet<string>;
  /** The user is deafened: every frame is muted. */
  readonly all: boolean;
}

let state: MuteState = { plugins: new Set(), all: false };
let boundApi: MumcordApi | undefined;
let unsubscribe: Unsubscribe | undefined;
const listeners = new Set<() => void>();

function applyChange(current: MuteState, plugin: string | null, muted: boolean): MuteState {
  if (plugin === null) return { ...current, all: muted };
  const plugins = new Set(current.plugins);
  if (muted) plugins.add(plugin);
  else plugins.delete(plugin);
  return { ...current, plugins };
}

function notify(): void {
  for (const listener of listeners) listener();
}

function ensureBound(): void {
  const api = getApi();
  if (api === boundApi) return;
  unsubscribe?.();
  boundApi = api;
  // Changes heard while the stored state is still on its way are newer than it.
  let pending: { plugin: string | null; muted: boolean }[] | null = [];
  unsubscribe = api.on("pluginFrameMute", ({ plugin, muted }) => {
    pending?.push({ plugin, muted });
    state = applyChange(state, plugin, muted);
    notify();
  });
  api
    .pluginFramesMuteState()
    .then((stored) => {
      if (api !== boundApi || !pending) return;
      let next: MuteState = { plugins: new Set(stored.plugins), all: stored.all };
      for (const change of pending) next = applyChange(next, change.plugin, change.muted);
      state = next;
      pending = null;
      notify();
    })
    .catch((error: unknown) => {
      console.error("Reading the plugin frames' mute state failed", toEngineError(error).code);
    })
    .finally(() => {
      pending = null;
    });
}

function subscribe(listener: () => void): () => void {
  ensureBound();
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): MuteState {
  ensureBound();
  return state;
}

export interface FrameMute {
  /** The user muted this plugin with its mute button. */
  readonly byUser: boolean;
  /** The page must be silent: muted by the user or deafened. */
  readonly muted: boolean;
}

export function useFrameMute(plugin: string): FrameMute {
  const current = useSyncExternalStore(subscribe, getSnapshot);
  const byUser = current.plugins.has(plugin);
  return { byUser, muted: byUser || current.all };
}

/** The mute button: asks Rust to tell every window (the state changes on its event). */
export function setPluginFramesMuted(plugin: string, muted: boolean): Promise<void> {
  return getApi()
    .pluginFramesMute(plugin, muted)
    .then(
      () => undefined,
      (error: unknown) => {
        console.error("Muting a plugin's frames failed", toEngineError(error).code);
        throw error;
      },
    );
}

/** Mutes all plugin frames while the user is deafened. Mount once, in the main window. */
export function usePluginFramesDeafen(): void {
  // Listens from the start, so frames that open later know about a mute that came first.
  useSyncExternalStore(subscribe, getSnapshot);
  const voiceId = useVoiceServerId();
  const self = selfUser(useServerStateOf(voiceId));
  const deafened = voiceId !== null && self !== undefined && (self.selfDeaf || self.serverDeaf);
  useEffect(() => {
    getApi()
      .pluginFramesMute(null, deafened)
      .catch((error: unknown) => {
        console.error("Muting the plugin frames failed", toEngineError(error).code);
      });
  }, [deafened]);
}

/** Test helper: back to the start. */
export function resetFrameMute(): void {
  unsubscribe?.();
  unsubscribe = undefined;
  boundApi = undefined;
  state = { plugins: new Set(), all: false };
}
