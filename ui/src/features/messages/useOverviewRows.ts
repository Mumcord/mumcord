import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useServers as useConnectedServers } from "../channels";
import { person } from "../follow/directEntries";
import { useFollowStates } from "../follow/followStore";
import { useFriends } from "../friends";
import { useIsBlocked } from "../blocking";
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

/** Every private conversation of every connected server except blocked people's, as rows. */
export function useOverviewRows(): Row[] {
  const { t } = useTranslation("chat");
  const follow = useFollowStates();
  const connected = useConnectedServers();
  const { servers } = useSavedServers();
  const friends = useFriends();
  const { last, prefs } = useConversationsState();
  const now = useNow();
  const isBlocked = useIsBlocked();

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
    const rows = buildRows({
      chats,
      last,
      prefs,
      friends: new Set(friends.map((f) => f.certHash)),
      liveName: (serverId, conversation) => {
        const users = connected.get(serverId)?.users;
        return users ? person(users, conversation)?.name : undefined;
      },
      serverName: (serverId) => {
        const server = servers.find((s) => s.id === serverId);
        return server ? displayName(server) : "";
      },
      unknownName: t("overview.unknown"),
      now,
    });
    // Conversations with blocked people are not shown (issue #140); unblocking brings them back.
    return rows.filter((row) => !isBlocked(row.serverId, row.certHash, row.name));
  }, [follow, connected, servers, friends, last, prefs, now, t, isBlocked]);
}
