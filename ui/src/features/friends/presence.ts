/** Text for a friend's online state (FR-1.64): where they are, or when they were last seen. */
import type { TFunction } from "i18next";
import type { Friend } from "../../api";
import type { ServerState } from "../channels";

export function lastSeenText(t: TFunction<"friends">, lastSeenAt: string | null, locale: string) {
  if (lastSeenAt === null) return t("offline");
  const when = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(lastSeenAt),
  );
  return t("lastSeen", { when });
}

export function presenceText(
  t: TFunction<"friends">,
  friend: Friend,
  server: ServerState,
  locale: string,
): string {
  if (friend.online === null) return lastSeenText(t, friend.lastSeenAt, locale);
  const channel = server.channels.get(friend.online.channelId)?.name;
  return channel ? t("onlineIn", { channel }) : t("online");
}
