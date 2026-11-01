import { getApi, type HotkeyAction } from "../../api";
import {
  getAllServerStates,
  getServerStateOf,
  getShownServerId,
  getVoiceServerId,
  selfUser,
  type ServerState,
} from "../channels/serverStore";
import { triggerUiHotkey } from "./hotkeyBus";

/** A channel voice was in, for "Back to previous channel". */
export interface VoicePlace {
  serverId: number;
  channelId: number;
}

/** The channel at `path` (names below the root), if the server has it. */
export function channelAt(state: ServerState, path: readonly string[]): number | null {
  const channels = [...state.channels.values()];
  let current = channels.find((c) => c.parentId === null);
  for (const name of path) {
    if (current === undefined) return null;
    const parent = current.id;
    current = channels.find((c) => c.parentId === parent && c.name === name);
  }
  return current?.id ?? null;
}

/** The names from below the root down to `channelId`, as stored by "switch to channel". */
export function channelPath(state: ServerState, channelId: number): string[] {
  const path: string[] = [];
  let channel = state.channels.get(channelId);
  while (channel !== undefined && channel.parentId !== null) {
    path.unshift(channel.name);
    channel = state.channels.get(channel.parentId);
  }
  return path;
}

/** The server whose own user mute and deafen apply to: voice, else the shown one. */
function ownServer(): number | null {
  return getVoiceServerId() ?? getShownServerId();
}

/** The next connected server after the voice server, with the own user's channel there. */
function nextVoicePlace(): VoicePlace | null {
  const connected = [...getAllServerStates()].filter(
    ([, state]) => state.connected && selfUser(state) !== undefined,
  );
  if (connected.length === 0) return null;
  const voice = getVoiceServerId();
  const at = connected.findIndex(([id]) => id === voice);
  const next = connected[(at + 1) % connected.length];
  if (next === undefined || next[0] === voice) return null;
  const [serverId, state] = next;
  const self = selfUser(state);
  return self === undefined ? null : { serverId, channelId: self.channelId };
}

/**
 * Runs a hotkey action (FR-3.72). Actions that do not apply right now (no connection, no
 * previous channel) do nothing; engine failures are logged, never shown as a dialog, since the
 * user may be in another app.
 */
export async function runHotkey(action: HotkeyAction, previous: VoicePlace | null): Promise<void> {
  const api = getApi();
  switch (action.kind) {
    case "toggleMute":
    case "toggleDeafen": {
      const id = ownServer();
      const self = id === null ? undefined : selfUser(getServerStateOf(id));
      if (self === undefined) return;
      if (action.kind === "toggleMute") await api.setSelfMute(!self.selfMute);
      else await api.setSelfDeaf(!self.selfDeaf);
      return;
    }
    case "toggleDnd": {
      const state = await api.dndState();
      await api.dndSet(!state.manual);
      return;
    }
    case "joinLeaveVoice": {
      if (getVoiceServerId() !== null) {
        await api.voiceLeave();
        return;
      }
      const shown = getShownServerId();
      const self = shown === null ? undefined : selfUser(getServerStateOf(shown));
      if (shown !== null && self !== undefined) await api.channelJoin(shown, self.channelId);
      return;
    }
    case "switchVoiceServer": {
      const next = nextVoicePlace();
      if (next !== null) await api.channelJoin(next.serverId, next.channelId);
      return;
    }
    case "switchChannel": {
      const state = getServerStateOf(action.serverId);
      const channelId = state.connected ? channelAt(state, action.path) : null;
      if (channelId !== null) await api.channelJoin(action.serverId, channelId);
      return;
    }
    case "previousChannel":
      if (previous !== null) await api.channelJoin(previous.serverId, previous.channelId);
      return;
    case "toggleMemberColumn":
    case "openQuickSettings":
      triggerUiHotkey(action.kind);
      return;
    case "openMumcord":
      // The engine brings the window to the front itself; in the window it is already there.
      return;
  }
}
