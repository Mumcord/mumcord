import type { ChannelInfo, User } from "../../api";
import type { ServerState } from "../channels/serverStore";

/**
 * Why "Join" can't enter the channel. Mumble reports a password channel (access token) the same
 * way as a channel the user may not enter, so "locked" covers both.
 */
export type JoinBlock = "locked" | "full";

/**
 * The channel "Join" would enter: the person's channel, unless it is the user's own. Friends on
 * another connected server never reach this menu (it only knows the active server's people).
 */
export function joinTarget(
  state: Pick<ServerState, "users" | "channels" | "selfSession">,
  user: User,
): { channel: ChannelInfo; blocked: JoinBlock | null } | null {
  const self = state.selfSession === null ? undefined : state.users.get(state.selfSession);
  if (user.session === state.selfSession || self?.channelId === user.channelId) return null;
  const channel = state.channels.get(user.channelId);
  if (!channel) return null;
  if (!channel.permissions.enter) return { channel, blocked: "locked" };
  const inside = [...state.users.values()].filter((u) => u.channelId === channel.id).length;
  const full = channel.maxUsers !== null && inside >= channel.maxUsers;
  return { channel, blocked: full ? "full" : null };
}
