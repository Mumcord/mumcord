/**
 * Whether the relay of `serverId` only serves registered users while this client is a guest
 * (#601). The bot announces it (`reg`), so the camera and file sharing can say why before trying.
 */
import { useServerStateOf } from "../channels";
import { useRelayInfo } from "./useRelayInfo";

export function useGuestRelayBlock(serverId: number | null): boolean {
  const info = useRelayInfo(serverId);
  const state = useServerStateOf(serverId);
  if (serverId === null || info === null || !info.registeredOnly) return false;
  const self = state.selfSession === null ? undefined : state.users.get(state.selfSession);
  return self !== undefined && !self.registered;
}
