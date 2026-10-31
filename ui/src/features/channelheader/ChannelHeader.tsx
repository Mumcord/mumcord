import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { useServerState } from "../channels";
import { ClearHistoryDialog } from "../chat/history/ClearHistory";
import { FollowToggle, useFollowControl } from "../follow";
import { EyeIcon } from "../follow/EyeIcon";
import { ServerMenu, type MenuItem } from "../servers/ServerMenu";
import { ChannelInfoDialog } from "./ChannelInfoDialog";
import { channelLink, channelPath, shortDescription } from "./channelModel";
import { ChevronIcon, InfoIcon, KeyIcon, LinkIcon, TrashIcon } from "./icons";
import "./channel-header.css";

/** How long "Link copied" stays in the header. */
const STATUS_MS = 3000;

/**
 * Left part of a channel's chat header (issue #170): the follow eye, the channel name as a menu
 * button (Follow, Copy channel link, Channel password, Channel info, Clear chat history) and a
 * line with the channel path and description.
 */
export function ChannelHeader({ channelId }: { channelId: number }) {
  const { t } = useTranslation("channelheader");
  const { t: tChat } = useTranslation("chat");
  const server = useServerState();
  const follow = useFollowControl(channelId);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const [dialog, setDialog] = useState<"info" | "clear" | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    if (status === null) return;
    const timer = window.setTimeout(() => {
      setStatus(null);
    }, STATUS_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [status]);

  const closeMenu = useCallback((returnFocus: boolean) => {
    setMenuAt(null);
    if (returnFocus) buttonRef.current?.focus();
  }, []);
  const closeDialog = useCallback(() => {
    setDialog(null);
    buttonRef.current?.focus();
  }, []);

  const channel = server.channels.get(channelId);
  const name = channel?.name ?? "";
  const path = channelPath(server.channels, channelId);
  const link = server.info ? channelLink(server.info, path) : null;
  const description = shortDescription(channel?.description ?? null);

  const openMenu = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    setMenuAt({ x: rect?.left ?? 0, y: (rect?.bottom ?? 0) + 4 });
  };

  const copyLink = async () => {
    if (link === null) return;
    try {
      await getApi().copyText(link);
      setStatus(t("copied"));
    } catch {
      // The copy API logs the real reason; the user only needs to know it did not work.
      setStatus(t("copyFailed"));
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

  const followItem: MenuItem = {
    id: "follow",
    label: follow.following ? t("menu.unfollow") : t("menu.follow"),
    icon: <EyeIcon />,
    ...(follow.following ? { hint: t("menu.following") } : {}),
    disabled: follow.busy,
    onSelect: () => {
      closeMenu(true);
      follow.toggle();
    },
  };
  const copyItem: MenuItem = {
    id: "copy-link",
    label: t("menu.copyLink"),
    icon: <LinkIcon />,
    hint: "mumble://",
    onSelect: () => {
      closeMenu(true);
      void copyLink();
    },
  };
  const passwordItem: MenuItem = {
    id: "password",
    label: t("menu.password"),
    icon: <KeyIcon />,
    onSelect: () => {
      closeMenu(true);
      enterPassword();
    },
  };
  // Items the user cannot use here are left out (FR-3.88).
  const items: MenuItem[] = [
    ...(follow.supported ? [followItem] : []),
    ...(link !== null ? [copyItem] : []),
    ...(channel?.enterRestricted && !channel.permissions.enter ? [passwordItem] : []),
    {
      id: "info",
      label: t("menu.info"),
      icon: <InfoIcon />,
      separatorBefore: true,
      onSelect: () => {
        setMenuAt(null);
        setDialog("info");
      },
    },
    {
      id: "clear",
      label: t("menu.clear"),
      icon: <TrashIcon />,
      danger: true,
      separatorBefore: true,
      disabled: server.serverId === null,
      onSelect: () => {
        setMenuAt(null);
        setDialog("clear");
      },
    },
  ];

  const onKeyDown = (event: KeyboardEvent) => {
    // Enter and Space click the button; the menu key, Shift+F10 and ArrowDown open it too.
    if (
      event.key === "ContextMenu" ||
      event.key === "ArrowDown" ||
      (event.key === "F10" && event.shiftKey)
    ) {
      event.preventDefault();
      openMenu();
    }
  };

  return (
    <div className="channel-header">
      <FollowToggle control={follow} />
      <h2 className="chat-header__title channel-header__title">
        <button
          ref={buttonRef}
          type="button"
          className="channel-header__name"
          aria-haspopup="menu"
          aria-expanded={menuAt !== null}
          onClick={() => {
            if (menuAt) closeMenu(false);
            else openMenu();
          }}
          onKeyDown={onKeyDown}
        >
          <span className="chat-header__hash" aria-hidden="true">
            #
          </span>
          <span className="visually-hidden">{tChat("header.channelPrefix")} </span>
          <span className="channel-header__text">{name}</span>
          <ChevronIcon />
        </button>
      </h2>
      {(path.length > 1 || description !== "") && (
        <p className="channel-header__subtitle">
          {path.length > 1 && <span>{path.join(" › ")}</span>}
          {path.length > 1 && description !== "" && <span aria-hidden="true"> · </span>}
          {description}
        </p>
      )}
      <span className="channel-header__status" role="status">
        {status}
      </span>
      {menuAt && (
        <ServerMenu
          label={t("menu.label", { channel: name })}
          position={menuAt}
          onClose={closeMenu}
          items={items}
        />
      )}
      {dialog === "info" && (
        <ChannelInfoDialog
          channelId={channelId}
          link={link}
          following={follow.supported ? follow.following : null}
          onClose={closeDialog}
        />
      )}
      {dialog === "clear" && (
        <ClearHistoryDialog conversation={{ kind: "channel", channelId }} onClose={closeDialog} />
      )}
    </div>
  );
}
