import type { ChannelInfo, User } from "../../api";
import type { ServerState } from "../channels/serverStore";

/**
 * The channel "Join" would enter: the person's channel, unless it is the user's own. Friends on
 * another connected server never reach this menu (it only knows the active server's people).
 */
export function joinTarget(
  state: Pick<ServerState, "users" | "channels" | "selfSession">,
  user: User,
): { channel: ChannelInfo; allowed: boolean } | null {
  const self = state.selfSession === null ? undefined : state.users.get(state.selfSession);
  if (user.session === state.selfSession || self?.channelId === user.channelId) return null;
  const channel = state.channels.get(user.channelId);
  return channel ? { channel, allowed: channel.permissions.enter } : null;
}
