import { useEffect, useState } from "react";
import { getApi, type ServerInfo, type User } from "../../api";
import { selfUser, useServerState } from "../channels/serverStore";

export interface UserPanelState {
  /** Own name from the identity; shown even while disconnected. */
  identityName: string | null;
  connected: boolean;
  self: User | null;
  info: ServerInfo | null;
  /** The server lets the own, not yet registered user register (root channel permission). */
  canRegister: boolean;
  /** Name of the own channel, for the read-only notice's join button. */
  channelName: string | null;
}

export function useUserPanel(): UserPanelState {
  const server = useServerState();
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
  const registered = self?.registered ?? true;
  const root = [...server.channels.values()].find((c) => c.parentId === null);
  return {
    identityName,
    connected,
    self,
    info: server.info,
    canRegister: connected && !registered && root?.permissions.registerSelf === true,
    channelName: self ? (server.channels.get(self.channelId)?.name ?? null) : null,
  };
}
