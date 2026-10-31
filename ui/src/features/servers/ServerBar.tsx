import {
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import type { SavedServer } from "../../api";
import { useShownServerId, useVoiceServerId } from "../channels";
import { showInfo, useHiddenHash, welcomeKey } from "../chat/hiddenInfo";
import { useFollowStates } from "../follow/followStore";
import { displayName, initials } from "./format";
import { mentionCount, useMentions } from "./mentions";
import { ServerMenu, type MenuItem } from "./ServerMenu";
import {
  disconnect,
  moveServer,
  openDialog,
  requestConnect,
  toggleAlwaysConnect,
  toggleGroupMentionsMuted,
  toggleNotificationsMuted,
  useServers,
} from "./store";
import { serverLabel, statusOf, type ServerBadges, type ServerStatus } from "./serverStatus";

interface ServerOptionsProps {
  server: SavedServer;
  index: number;
  count: number;
  status: ServerStatus;
  position: { x: number; y: number };
  onMove: (server: SavedServer, toIndex: number) => void;
  onClose: (returnFocus: boolean) => void;
}

/** Context menu of one server icon. */
function ServerOptions({
  server,
  index,
  count,
  status,
  position,
  onMove,
  onClose,
}: ServerOptionsProps) {
  const { t } = useTranslation("servers");
  // Each action first returns focus to the icon, so dialogs restore focus there on close.
  const act = (fn: () => void) => () => {
    onClose(true);
    fn();
  };
  const hiddenWelcome = useHiddenHash(welcomeKey(server.id));
  const connected = status !== null && status !== "error" && status !== "failed";
  const items: MenuItem[] = [
    connected
      ? {
          id: "disconnect",
          label: t("menu.disconnect"),
          onSelect: act(() => void disconnect(server.id)),
        }
      : {
          id: "connect",
          label:
            status === "error" || status === "failed" ? t("menu.reconnect") : t("menu.connect"),
          onSelect: act(() => {
            requestConnect(server.id);
          }),
        },
    {
      id: "alwaysConnect",
      label: t("menu.alwaysConnect"),
      checked: server.alwaysConnectAtStart,
      onSelect: act(() => void toggleAlwaysConnect(server.id, !server.alwaysConnectAtStart)),
    },
    {
      id: "muteNotifications",
      label: t("menu.muteNotifications"),
      checked: server.notificationsMuted,
      onSelect: act(() => void toggleNotificationsMuted(server.id, !server.notificationsMuted)),
    },
    {
      id: "muteGroupMentions",
      label: t("menu.muteGroupMentions"),
      checked: server.groupMentionsMuted,
      onSelect: act(() => void toggleGroupMentionsMuted(server.id, !server.groupMentionsMuted)),
    },
    {
      id: "identity",
      label: t("menu.identity"),
      onSelect: act(() => {
        openDialog({ kind: "identity", serverId: server.id });
      }),
    },
    {
      id: "theme",
      label: t("menu.theme"),
      onSelect: act(() => {
        openDialog({ kind: "theme", serverId: server.id });
      }),
    },
    {
      id: "edit",
      label: t("menu.edit"),
      onSelect: act(() => {
        openDialog({ kind: "edit", serverId: server.id });
      }),
    },
  ];
  if (index > 0) {
    items.push({
      id: "up",
      label: t("menu.moveUp"),
      onSelect: act(() => {
        onMove(server, index - 1);
      }),
    });
  }
  if (index < count - 1) {
    items.push({
      id: "down",
      label: t("menu.moveDown"),
      onSelect: act(() => {
        onMove(server, index + 1);
      }),
    });
  }
  if (hiddenWelcome !== null) {
    items.push({
      id: "showWelcome",
      label: t("menu.showWelcome"),
      onSelect: act(() => {
        showInfo(welcomeKey(server.id));
      }),
    });
  }
  items.push({
    id: "remove",
    label: t("menu.remove"),
    danger: true,
    onSelect: act(() => {
      openDialog({ kind: "remove", serverId: server.id });
    }),
  });
  return (
    <ServerMenu
      label={t("menu.label", { server: displayName(server) })}
      items={items}
      position={position}
      onClose={onClose}
    />
  );
}

/**
 * Left column: saved servers as round icons with their state (connected, connecting, error,
 * voice server, unread, mentions; spec §5.2), "+" to add. The shown server is marked.
 * One Tab stop with arrow-key navigation; Alt+Up/Down or drag and drop reorders.
 */
export interface ServerBarProps {
  /** Shown above the servers, e.g. the private messages button (issue #173). */
  top?: ReactNode;
  /** Something from `top` fills the channel column: no server is marked as shown. */
  topActive?: boolean;
  /** A server was clicked (to leave what `top` opened). */
  onServerClick?: () => void;
}

export function ServerBar({ top, topActive = false, onServerClick }: ServerBarProps = {}) {
  const { t } = useTranslation("servers");
  const state = useServers();
  const shownId = useShownServerId();
  const voiceId = useVoiceServerId();
  const follow = useFollowStates();
  const mentions = useMentions();
  const { servers } = state;
  const badgesOf = (id: number): ServerBadges => {
    const unread = [...(follow.get(id)?.unread.values() ?? [])].some((count) => count > 0);
    return {
      status: statusOf(state, id),
      voice: voiceId === id,
      unread,
      mentions: mentionCount(mentions.get(id), follow.get(id)),
    };
  };
  const hintId = useId();
  const [focusIndex, setFocusIndex] = useState(0);
  const [menu, setMenu] = useState<{ serverId: number; x: number; y: number } | null>(null);
  const [dragId, setDragId] = useState<number | null>(null);
  const buttons = useRef(new Map<number, HTMLButtonElement>());
  // The open menu's server, read by the stable close callback without re-creating it.
  const menuServerId = useRef<number | null>(null);
  const activeIndex = Math.min(focusIndex, servers.length - 1);

  const focusServer = (index: number) => {
    const server = servers[index];
    if (!server) return;
    setFocusIndex(index);
    buttons.current.get(server.id)?.focus();
  };

  // Moving a focused element in the DOM can drop its focus; put it back after a reorder.
  const refocusId = useRef<number | null>(null);
  useLayoutEffect(() => {
    const id = refocusId.current;
    refocusId.current = null;
    if (id !== null) buttons.current.get(id)?.focus();
  }, [servers]);

  const move = (server: SavedServer, toIndex: number) => {
    setFocusIndex(toIndex);
    if (document.activeElement === buttons.current.get(server.id)) refocusId.current = server.id;
    void moveServer(server.id, toIndex);
  };

  const openMenu = (server: SavedServer, at?: { x: number; y: number }) => {
    const rect = buttons.current.get(server.id)?.getBoundingClientRect();
    menuServerId.current = server.id;
    setMenu({ serverId: server.id, x: at?.x ?? rect?.right ?? 0, y: at?.y ?? rect?.top ?? 0 });
  };

  const closeMenu = useCallback((returnFocus: boolean) => {
    const id = menuServerId.current;
    menuServerId.current = null;
    setMenu(null);
    if (returnFocus && id !== null) buttons.current.get(id)?.focus();
  }, []);

  const onKeyDown = (event: KeyboardEvent, server: SavedServer, index: number) => {
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      const target = index + (event.key === "ArrowUp" ? -1 : 1);
      if (target >= 0 && target < servers.length) move(server, target);
    } else if (event.key === "ArrowDown") focusServer(Math.min(index + 1, servers.length - 1));
    else if (event.key === "ArrowUp") focusServer(Math.max(index - 1, 0));
    else if (event.key === "Home") focusServer(0);
    else if (event.key === "End") focusServer(servers.length - 1);
    else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      openMenu(server);
    } else if (event.key === "Delete") openDialog({ kind: "remove", serverId: server.id });
    else return;
    event.preventDefault();
  };

  const onDrop = (event: DragEvent, index: number) => {
    event.preventDefault();
    const dragged = servers.find((s) => s.id === dragId);
    if (dragged) move(dragged, index);
    setDragId(null);
  };

  const menuIndex = menu ? servers.findIndex((s) => s.id === menu.serverId) : -1;
  const menuServer = servers[menuIndex];

  return (
    <div className="server-bar">
      <p id={hintId} className="visually-hidden">
        {t("bar.keyboardHint")}
      </p>
      {top}
      <ul className="server-bar__list" aria-label={t("bar.listLabel")}>
        {servers.map((server, index) => {
          const badges = badgesOf(server.id);
          const { status } = badges;
          const name = displayName(server);
          const label = serverLabel(t, name, badges);
          const shown =
            !topActive &&
            shownId === server.id &&
            status !== null &&
            status !== "error" &&
            status !== "failed";
          return (
            <li
              key={server.id}
              className="server-bar__item"
              data-status={status ?? undefined}
              data-shown={shown || undefined}
              data-dragging={dragId === server.id || undefined}
            >
              <button
                ref={(element) => {
                  if (element) buttons.current.set(server.id, element);
                  else buttons.current.delete(server.id);
                }}
                type="button"
                className="server-bar__server"
                tabIndex={index === activeIndex ? 0 : -1}
                aria-label={label}
                data-tooltip={label}
                data-tooltip-placement="right"
                aria-current={shown ? "true" : undefined}
                aria-describedby={hintId}
                draggable
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = "move";
                  event.dataTransfer.setData("text/plain", String(server.id));
                  setDragId(server.id);
                }}
                onDragOver={(event) => {
                  if (dragId !== null) event.preventDefault();
                }}
                onDrop={(event) => {
                  onDrop(event, index);
                }}
                onDragEnd={() => {
                  setDragId(null);
                }}
                onFocus={() => {
                  setFocusIndex(index);
                }}
                onClick={() => {
                  onServerClick?.();
                  requestConnect(server.id);
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  openMenu(server, { x: event.clientX, y: event.clientY });
                }}
                onKeyDown={(event) => {
                  onKeyDown(event, server, index);
                }}
              >
                <span aria-hidden="true">{initials(name)}</span>
                <ServerMarks badges={badges} />
              </button>
            </li>
          );
        })}
      </ul>
      <div className="server-bar__item">
        <button
          type="button"
          className="server-bar__server server-bar__add"
          aria-label={t("bar.add")}
          data-tooltip={t("bar.add")}
          data-tooltip-placement="right"
          onClick={() => {
            openDialog({ kind: "add", link: null });
          }}
        >
          <span aria-hidden="true">+</span>
        </button>
      </div>
      {menu && menuServer && (
        <ServerOptions
          server={menuServer}
          index={menuIndex}
          count={servers.length}
          status={statusOf(state, menuServer.id)}
          position={{ x: menu.x, y: menu.y }}
          onMove={move}
          onClose={closeMenu}
        />
      )}
    </div>
  );
}

/** Visual marks on an icon; hidden from screen readers, the button's label says the same. */
function ServerMarks({ badges }: { badges: ServerBadges }) {
  const { status } = badges;
  return (
    <>
      {badges.voice ? (
        <span className="server-bar__mark server-bar__mark--voice" aria-hidden="true">
          <svg viewBox="0 0 24 24" width="12" height="12" fill="currentColor">
            <path d="M4 9v6h4l5 4V5L8 9H4zm12.5 3a4.5 4.5 0 0 0-2.5-4v8a4.5 4.5 0 0 0 2.5-4z" />
          </svg>
        </span>
      ) : null}
      {status === "connecting" || status === "reconnecting" ? (
        <span className="server-bar__mark server-bar__mark--progress" aria-hidden="true">
          ⟳
        </span>
      ) : status === "error" || status === "failed" ? (
        <span className="server-bar__mark server-bar__mark--error" aria-hidden="true">
          !
        </span>
      ) : null}
      {badges.mentions > 0 ? (
        <span className="server-bar__mark server-bar__mark--mentions" aria-hidden="true">
          {badges.mentions > 99 ? "99+" : badges.mentions}
        </span>
      ) : badges.unread ? (
        <span className="server-bar__mark server-bar__mark--unread" aria-hidden="true" />
      ) : null}
    </>
  );
}
