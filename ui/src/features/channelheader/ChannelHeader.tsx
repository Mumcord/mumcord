import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { useServerState } from "../channels";
import { profileKind, useChannelAudioProfile } from "../channelaudio";
import { FollowToggle } from "../follow";
import { ServerMenu } from "../servers/ServerMenu";
import { useChannelActions } from "./channelActions";
import { channelPath, shortDescription } from "./channelModel";
import { ChevronIcon } from "./icons";
import "./channel-header.css";

/** How long "Link copied" stays in the header. */
const STATUS_MS = 3000;

/**
 * Left part of a channel's chat header (issue #170): the follow eye, the channel name as a menu
 * button (the shared channel actions, see `useChannelActions`), a line with the channel path and
 * description and the ♪ / ⚡ badge of a channel audio profile (issue #171).
 */
export function ChannelHeader({ channelId }: { channelId: number }) {
  const { t } = useTranslation("channelheader");
  const { t: tChat } = useTranslation("chat");
  const server = useServerState();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const audioKind = profileKind(useChannelAudioProfile(server.serverId, channelId));
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
  const actions = useChannelActions(channelId, {
    context: "header",
    closeMenu,
    restoreFocus: () => buttonRef.current?.focus(),
    onStatus: setStatus,
  });

  const channel = server.channels.get(channelId);
  const name = channel?.name ?? "";
  const path = channelPath(server.channels, channelId);
  const description = shortDescription(channel?.description ?? null);

  const openMenu = () => {
    const rect = buttonRef.current?.getBoundingClientRect();
    setMenuAt({ x: rect?.left ?? 0, y: (rect?.bottom ?? 0) + 4 });
  };

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
      <FollowToggle control={actions.follow} />
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
      {audioKind && (
        <span className={`channel-header__badge channel-header__badge--${audioKind}`}>
          <span aria-hidden="true">{audioKind === "jam" ? "⚡" : "♪"} </span>
          {t(`badge.${audioKind}`)}
        </span>
      )}
      <span className="channel-header__status" role="status">
        {status}
      </span>
      {menuAt && (
        <ServerMenu
          label={actions.label}
          position={menuAt}
          onClose={closeMenu}
          items={actions.items}
        />
      )}
      {actions.dialog}
    </div>
  );
}
