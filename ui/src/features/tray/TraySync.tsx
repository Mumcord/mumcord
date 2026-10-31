import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type User } from "../../api";
import { useServers as useConnectedServers } from "../channels";
import { personName, type Entry } from "../follow/directEntries";
import { useFollowStates } from "../follow/followStore";
import { useFriends } from "../friends";
import { useServers as useSavedServers } from "../servers";
import { useMentions } from "../servers/mentions";
import { trayAttention } from "./trayAttention";

export interface TraySyncProps {
  /** The tray menu's "Settings…" was chosen. */
  onOpenSettings: () => void;
}

/** Keeps the tray's unread badge and recent chats current and opens the settings from it. */
export function TraySync({ onOpenSettings }: TraySyncProps) {
  const { t } = useTranslation("follow");
  const states = useFollowStates();
  const mentions = useMentions();
  const { servers } = useSavedServers();
  const friends = useFriends();
  const connected = useConnectedServers();

  const nameOf = (entry: Entry): string => {
    const c = entry.chat.conversation;
    const users = connected.get(entry.serverId)?.users ?? new Map<number, User>();
    const friend = c.kind === "private" ? friends.find((f) => f.certHash === c.certHash) : null;
    return personName(users, c) ?? entry.chat.name ?? friend?.name ?? t("dm.unknown");
  };
  // Compared as text so the tray only hears about real changes.
  const summary = JSON.stringify(trayAttention({ states, mentions, servers, friends, nameOf }));

  useEffect(() => {
    getApi()
      .trayAttentionSet(JSON.parse(summary) as ReturnType<typeof trayAttention>)
      .catch((error: unknown) => {
        console.error("Updating the tray failed", error);
      });
  }, [summary]);

  useEffect(
    () =>
      getApi().on("traySettingsRequested", () => {
        onOpenSettings();
      }),
    [onOpenSettings],
  );

  return null;
}
