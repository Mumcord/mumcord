import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Conversation, User } from "../../api";
import { useServers as useConnectedServers } from "../channels";
import { useFollowStates } from "../follow/followStore";
import { useFriends } from "../friends";
import { displayName } from "../servers/format";
import { useServers as useSavedServers } from "../servers/store";
import { useConversationsState } from "./conversationsStore";
import { buildRows, type OpenChat, type Row } from "./overviewModel";

/** How often timed mutes are checked for their end. */
const CLOCK_MS = 30_000;

export function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, CLOCK_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);
  return now;
}

/** The person of a private chat among the users of a server. */
export function personOf(users: ReadonlyMap<number, User>, c: Conversation): User | undefined {
  if (c.kind === "private") return [...users.values()].find((u) => u.certHash === c.certHash);
  if (c.kind === "privateSession") return users.get(c.session);
  return undefined;
}

/** Every private conversation of every connected server, as overview rows. */
export function useOverviewRows(): Row[] {
  const { t } = useTranslation("chat");
  const follow = useFollowStates();
  const connected = useConnectedServers();
  const { servers } = useSavedServers();
  const friends = useFriends();
  const { last, prefs } = useConversationsState();
  const now = useNow();

  return useMemo(() => {
    const chats: OpenChat[] = [];
    for (const [serverId, state] of follow) {
      for (const chat of state.chats) {
        chats.push({
          serverId,
          conversation: chat.conversation,
          name: chat.name,
          unread: state.unread.get(chat.key) ?? 0,
        });
      }
    }
    return buildRows({
      chats,
      last,
      prefs,
      friends: new Set(friends.map((f) => f.certHash)),
      liveName: (serverId, conversation) => {
        const users = connected.get(serverId)?.users;
        return users ? personOf(users, conversation)?.name : undefined;
      },
      serverName: (serverId) => {
        const server = servers.find((s) => s.id === serverId);
        return server ? displayName(server) : "";
      },
      unknownName: t("overview.unknown"),
      now,
    });
  }, [follow, connected, servers, friends, last, prefs, now, t]);
}
