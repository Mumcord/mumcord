/** Text for a friend's online state (FR-1.64): where they are, or when they were last seen. */
import type { TFunction } from "i18next";
import type { Friend } from "../../api";

export function lastSeenText(t: TFunction<"friends">, lastSeenAt: string | null, locale: string) {
  if (lastSeenAt === null) return t("offline");
  const when = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(lastSeenAt),
  );
  return t("lastSeen", { when });
}

/** Where friends can be looked up: server names and, for connected servers, channel names. */
export interface PresenceLookup {
  shownServerId: number | null;
  serverName: (serverId: number) => string | undefined;
  channelName: (serverId: number, channelId: number) => string | undefined;
}

export function presenceText(
  t: TFunction<"friends">,
  friend: Friend,
  locale: string,
  lookup: PresenceLookup,
): string {
  if (friend.online === null) return lastSeenText(t, friend.lastSeenAt, locale);
  const { serverId, channelId } = friend.online;
  const channel = lookup.channelName(serverId, channelId);
  if (serverId === lookup.shownServerId) {
    return channel ? t("onlineIn", { channel }) : t("online");
  }
  const server = lookup.serverName(serverId);
  if (server === undefined) return t("online");
  return channel ? t("inChannelOnServer", { channel, server }) : t("onlineOnServer", { server });
}
