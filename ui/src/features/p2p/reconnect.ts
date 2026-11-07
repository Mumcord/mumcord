/** Reconnecting to the same channel, the way out of a bot that lost this client's hello (#601). */
import { getServerStateOf } from "../channels";
import { disconnect, requestConnect } from "../servers/store";

/** The channel path below the root, as `connect` takes it, to come back to the same channel. */
function channelPath(serverId: number): string[] | null {
  const state = getServerStateOf(serverId);
  const self = state.selfSession === null ? undefined : state.users.get(state.selfSession);
  if (!self) return null;
  const names: string[] = [];
  let channel = state.channels.get(self.channelId);
  while (channel && channel.parentId !== null) {
    names.unshift(channel.name);
    channel = state.channels.get(channel.parentId);
  }
  return names;
}

/** A clean reconnect sends the hello the bot is missing. */
export async function reconnectKeepingChannel(serverId: number): Promise<void> {
  const path = channelPath(serverId);
  await disconnect(serverId);
  requestConnect(serverId, path);
}
