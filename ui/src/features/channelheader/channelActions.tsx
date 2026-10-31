import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerState } from "../channels";
import { ChannelAudioDialog, profileKind, useChannelAudioProfile } from "../channelaudio";
import { ClearHistoryDialog } from "../chat/history/ClearHistory";
import { useFollowControl, type FollowControl } from "../follow";
import { EyeIcon } from "../follow/EyeIcon";
import type { MenuItem } from "../servers/ServerMenu";
import { ChannelInfoDialog } from "./ChannelInfoDialog";
import { channelLink, channelPath } from "./channelModel";
import {
  EraserIcon,
  EyeOffIcon,
  InfoIcon,
  JoinIcon,
  KeyIcon,
  LinkIcon,
  MusicIcon,
  PeopleIcon,
} from "./icons";
import { MoveUsersDialog } from "./MoveUsersDialog";
import { WhisperKindIcon } from "../whisper/WhisperKindIcon";
import { createWhisperTarget } from "../whisper/whisperStore";

/** Where the menu opens: the channel name in the chat header, or a channel row in the tree. */
export type ChannelActionsContext = "header" | "tree";

export interface ChannelActionsOptions {
  context: ChannelActionsContext;
  /** Closes the menu; `returnFocus` puts focus back on what opened it. */
  closeMenu: (returnFocus: boolean) => void;
  /** Puts focus back on what opened the menu once a dialog closes. */
  restoreFocus: () => void;
  /** Short feedback such as "Link copied". */
  onStatus: (text: string) => void;
  /** Joins the channel (tree only); left out while the user is already in it. */
  join?: (() => void) | undefined;
}

export interface ChannelActions {
  /** The menu's accessible name. */
  label: string;
  items: MenuItem[];
  /** The dialog an item opened, if any; render it next to the menu. */
  dialog: ReactNode;
  follow: FollowControl;
}

type DialogKind = "info" | "audio" | "clear" | "move";

/**
 * The one list of channel actions (issues #170, #172), shared by the channel-name menu in the
 * chat header and the channel's right-click menu in the tree, so both always offer the same
 * things. The tree adds "Join channel" at the top and "Move users here…" before "Clear history".
 * Items the user may not use are left out, not disabled (FR-3.84, FR-3.88).
 */
export function useChannelActions(
  channelId: number,
  { context, closeMenu, restoreFocus, onStatus, join }: ChannelActionsOptions,
): ChannelActions {
  const { t } = useTranslation("channelheader");
  const { t: tWhisper } = useTranslation("whisper");
  const server = useServerState();
  const follow = useFollowControl(channelId);
  const audioKind = profileKind(useChannelAudioProfile(server.serverId, channelId));
  const [dialog, setDialog] = useState<DialogKind | null>(null);

  const channel = server.channels.get(channelId);
  const name = channel?.name ?? "";
  const link = server.info
    ? channelLink(server.info, channelPath(server.channels, channelId))
    : null;
  const connected = server.serverId !== null;

  const openDialog = (kind: DialogKind) => {
    closeMenu(false);
    setDialog(kind);
  };
  const closeDialog = () => {
    setDialog(null);
    restoreFocus();
  };

  const copyLink = async () => {
    if (link === null) return;
    try {
      await getApi().copyText(link);
      onStatus(t("copied"));
    } catch {
      // The copy API logs the real reason; the user only needs to know it did not work.
      onStatus(t("copyFailed"));
    }
  };

  const enterPassword = () => {
    if (server.serverId === null) return;
    // A refused join makes the engine ask for the password (FR-2.78).
    getApi()
      .channelJoin(server.serverId, channelId)
      .catch((error: unknown) => {
        console.error("Joining a protected channel failed", error);
      });
  };

  const joinItem: MenuItem | null = join
    ? {
        id: "join",
        label: t("menu.join"),
        icon: <JoinIcon />,
        hint: "Shift+Enter",
        onSelect: () => {
          closeMenu(true);
          join();
        },
      }
    : null;
  const followItem: MenuItem | null = follow.supported
    ? {
        id: "follow",
        label: follow.following ? t("menu.unfollow") : t("menu.follow"),
        // The icon shows the action, like the label: an open eye to follow, a crossed one to stop.
        icon: follow.following ? <EyeOffIcon /> : <EyeIcon />,
        ...(follow.following ? { hint: t("menu.following") } : {}),
        disabled: follow.busy,
        onSelect: () => {
          closeMenu(true);
          follow.toggle();
        },
      }
    : null;
  const audioItem: MenuItem | null = connected
    ? {
        id: "audio",
        label: t("menu.audio"),
        icon: <MusicIcon />,
        ...(audioKind ? { hint: t(`menu.audioHint.${audioKind}`) } : {}),
        onSelect: () => {
          openDialog("audio");
        },
      }
    : null;
  // Shouting needs the Whisper permission in the channel (the server checks it, too); like the
  // other actions, the item is hidden without it.
  const whisperItem: MenuItem | null =
    connected && channel && channel.parentId !== null && channel.permissions.whisper
      ? {
          id: "whisper",
          label: tWhisper("menu.whisper"),
          icon: <WhisperKindIcon kind="shout" />,
          onSelect: () => {
            closeMenu(false);
            createWhisperTarget({
              name,
              receivers: {
                kind: "channel",
                path: channelPath(server.channels, channelId),
                children: false,
                links: false,
                group: null,
              },
            });
          },
        }
      : null;
  const copyItem: MenuItem | null =
    link !== null
      ? {
          id: "copy-link",
          label: t("menu.copyLink"),
          icon: <LinkIcon />,
          hint: "mumble://",
          onSelect: () => {
            closeMenu(true);
            void copyLink();
          },
        }
      : null;
  const passwordItem: MenuItem | null =
    channel?.enterRestricted && !channel.permissions.enter
      ? {
          id: "password",
          label: t("menu.password"),
          icon: <KeyIcon />,
          onSelect: () => {
            closeMenu(true);
            enterPassword();
          },
        }
      : null;
  const infoItem: MenuItem = {
    id: "info",
    label: t("menu.info"),
    icon: <InfoIcon />,
    separatorBefore: true,
    onSelect: () => {
      openDialog("info");
    },
  };
  const moveItem: MenuItem | null =
    context === "tree" && connected && channel?.permissions.moveUsers
      ? {
          id: "move",
          label: t("menu.move"),
          icon: <PeopleIcon />,
          separatorBefore: true,
          onSelect: () => {
            openDialog("move");
          },
        }
      : null;
  const clearItem: MenuItem | null = connected
    ? {
        id: "clear",
        label: t("menu.clear"),
        icon: <EraserIcon />,
        danger: true,
        separatorBefore: moveItem === null,
        onSelect: () => {
          openDialog("clear");
        },
      }
    : null;

  const ordered = [
    joinItem,
    followItem,
    audioItem,
    whisperItem,
    copyItem,
    passwordItem,
    infoItem,
    moveItem,
    clearItem,
  ].filter((item): item is MenuItem => item !== null);
  // "Join channel" stands apart from the rest, like in the mockup.
  const items = ordered.map((item, index) =>
    index === 1 && joinItem !== null ? { ...item, separatorBefore: true } : item,
  );

  let dialogNode: ReactNode = null;
  if (dialog === "info") {
    dialogNode = (
      <ChannelInfoDialog
        channelId={channelId}
        link={link}
        following={follow.supported ? follow.following : null}
        onOpenAudio={() => {
          setDialog("audio");
        }}
        onClose={closeDialog}
      />
    );
  } else if (dialog === "audio") {
    dialogNode = <ChannelAudioDialog channelId={channelId} onClose={closeDialog} />;
  } else if (dialog === "clear") {
    dialogNode = (
      <ClearHistoryDialog conversation={{ kind: "channel", channelId }} onClose={closeDialog} />
    );
  } else if (dialog === "move") {
    dialogNode = <MoveUsersDialog channelId={channelId} onClose={closeDialog} />;
  }

  return { label: t("menu.label", { channel: name }), items, dialog: dialogNode, follow };
}
