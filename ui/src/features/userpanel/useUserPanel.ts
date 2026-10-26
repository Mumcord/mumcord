import { useEffect, useState } from "react";
import { getApi, type ServerInfo, type User } from "../../api";
import {
  selfUser,
  useServerState,
  useServerStateOf,
  useShownServerId,
  useVoiceServerId,
} from "../channels/serverStore";

export interface UserPanelState {
  /** Own name from the identity; shown even while disconnected. */
  identityName: string | null;
  /** The voice server, or the shown server while no server has voice, is connected and synced. */
  connected: boolean;
  /** Own user on the voice server (where mute, deafen and talking apply), else on the shown one. */
  self: User | null;
  /** Own user on the shown server (profile, register, the read-only notice's join button). */
  shownSelf: User | null;
  /** The voice server with its own channel (FR-2.5); `null` while no server has voice. */
  voice: { serverId: number; channelName: string | null } | null;
  info: ServerInfo | null;
  /** The server lets the own, not yet registered user register (root channel permission). */
  canRegister: boolean;
  /** Name of the own channel on the shown server, for the read-only notice's join button. */
  channelName: string | null;
  /** Sessions currently transmitting or talking (own user included). */
  talking: ReadonlySet<number>;
}

export function useUserPanel(): UserPanelState {
  const shown = useServerState();
  const shownId = useShownServerId();
  const voiceId = useVoiceServerId();
  // The voice server's state; while there is none this is the shown server's.
  const server = useServerStateOf(voiceId ?? shownId);
  const [identityName, setIdentityName] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .identityGet()
      .then((identity) => {
        if (active) setIdentityName(identity?.name ?? null);
      })
      .catch((error: unknown) => {
        console.error("User panel could not load the identity", error);
      });
    return () => {
      active = false;
    };
  }, []);

  const self = selfUser(server) ?? null;
  const connected = server.connection.state === "connected" && self !== null;
  const shownSelf = selfUser(shown) ?? null;
  const shownConnected = shown.connection.state === "connected" && shownSelf !== null;
  const registered = shownSelf?.registered ?? true;
  const root = [...shown.channels.values()].find((c) => c.parentId === null);
  return {
    identityName,
    connected,
    self,
    shownSelf,
    voice:
      voiceId === null
        ? null
        : {
            serverId: voiceId,
            channelName: self ? (server.channels.get(self.channelId)?.name ?? null) : null,
          },
    info: shown.info,
    canRegister: shownConnected && !registered && root?.permissions.registerSelf === true,
    talking: server.talking,
    channelName: shownSelf ? (shown.channels.get(shownSelf.channelId)?.name ?? null) : null,
  };
}
