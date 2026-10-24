/** Channel column content: the channel tree of the connected server, or an empty state. */
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError } from "../../api";
import { useProfileCard } from "../members/useProfileCard";
import type { SendPrivateMessage } from "../members/privateMessage";
import { joinChannel } from "../userpanel/readOnly";
import { useServerAnnouncements } from "./announcements";
import { ChannelTree, type ChannelAction } from "./ChannelTree";
import { useSelectedConversation } from "./selection";
import { useServerState } from "./serverStore";

export interface ChannelsPanelProps {
  /** Extra content after a channel's people count, e.g. the unread badge (B22). */
  channelBadge?: ((channelId: number) => ReactNode) | undefined;
  /** Button in a fixed slot of each channel row, e.g. the follow star (QA.13). */
  channelAction?: ((channel: ChannelAction) => ReactNode) | undefined;
  /** Row above the tree, e.g. the server name with its menu (QA.13). */
  header?: ReactNode;
  /** "Send private message" on the profile card; defaults to opening the private chat. */
  onSendPrivateMessage?: SendPrivateMessage | undefined;
}

export function ChannelsPanel({
  channelBadge,
  channelAction,
  header,
  onSendPrivateMessage,
}: ChannelsPanelProps) {
  const { t } = useTranslation("channels");
  const state = useServerState();
  const { conversation, select } = useSelectedConversation();
  const { openProfile, profileCard } = useProfileCard(onSendPrivateMessage);
  const [joinError, setJoinError] = useState<string | null>(null);
  useServerAnnouncements();

  useEffect(() => {
    if (import.meta.env.DEV) {
      void import("./devConnect").then((m) => {
        m.devAutoConnect();
      });
    }
  }, []);

  const join = useCallback(
    (channelId: number) => {
      setJoinError(null);
      joinChannel(channelId).catch((cause: unknown) => {
        const { code } = toEngineError(cause);
        if (code !== "permissionDenied" && code !== "notConnected") {
          console.error("Joining a channel failed", code);
        }
        setJoinError(
          code === "permissionDenied"
            ? t("errors.joinDenied")
            : code === "notConnected"
              ? t("errors.notConnected")
              : t("errors.joinFailed"),
        );
      });
    },
    [t],
  );

  if (state.serverId === null) {
    return (
      <p className="channels-empty">
        {state.connection.state === "disconnected" ? t("empty.disconnected") : t("empty.loading")}
      </p>
    );
  }

  return (
    <div className="channels-panel">
      {header}
      <ChannelTree
        state={state}
        selectedChannelId={conversation?.kind === "channel" ? conversation.channelId : null}
        onSelect={(channelId) => {
          setJoinError(null);
          select({ kind: "channel", channelId });
        }}
        onJoin={join}
        onOpenPerson={openProfile}
        channelBadge={channelBadge}
        channelAction={channelAction}
      />
      {joinError && (
        <p role="alert" className="channels-error">
          {joinError}
        </p>
      )}
      {profileCard}
    </div>
  );
}
