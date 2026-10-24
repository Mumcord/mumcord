import {
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import type { SavedServer } from "../../api";
import { useServerState } from "../channels";
import { displayName, initials } from "./format";
import { ServerMenu, type MenuItem } from "./ServerMenu";
import {
  disconnect,
  moveServer,
  openDialog,
  requestConnect,
  useServers,
  type ServersState,
} from "./store";

type ServerStatus = "connected" | "connecting" | "reconnecting" | null;

/**
 * Status of one icon. "Connected" follows the shared server state (synced server), the
 * transitional states come from the own connection status.
 */
function statusOf(state: ServersState, connectedId: number | null, id: number): ServerStatus {
  const { connection } = state;
  if (connection.state === "connecting" || connection.state === "reconnecting") {
    return connection.serverId === id ? connection.state : null;
  }
  if (connection.state === "connected" && connection.serverId === id) return "connected";
  return connectedId === id && connection.state !== "disconnected" ? "connected" : null;
}

interface ServerOptionsProps {
  server: SavedServer;
  index: number;
  count: number;
  connected: boolean;
  position: { x: number; y: number };
  onMove: (server: SavedServer, toIndex: number) => void;
  onClose: (returnFocus: boolean) => void;
}

/** Context menu of one server icon. */
function ServerOptions({
  server,
  index,
  count,
  connected,
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
  const items: MenuItem[] = [
    connected
      ? {
          id: "disconnect",
          label: t("menu.disconnect"),
          onSelect: act(() => void disconnect()),
        }
      : {
          id: "connect",
          label: t("menu.connect"),
          onSelect: act(() => {
            requestConnect(server.id);
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
 * Left column: saved servers as round icons (connected one highlighted), "+" to add.
 * One Tab stop with arrow-key navigation; Alt+Up/Down or drag and drop reorders.
 */
export function ServerBar() {
  const { t } = useTranslation("servers");
  const state = useServers();
  const connectedId = useServerState().serverId;
  const { servers } = state;
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
      <ul className="server-bar__list" aria-label={t("bar.listLabel")}>
        {servers.map((server, index) => {
          const status = statusOf(state, connectedId, server.id);
          const name = displayName(server);
          return (
            <li
              key={server.id}
              className="server-bar__item"
              data-status={status ?? undefined}
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
                aria-label={status ? t(`bar.status.${status}`, { server: name }) : name}
                aria-current={status === "connected" ? "true" : undefined}
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
              </button>
              <span className="server-bar__tooltip" aria-hidden="true">
                {name}
              </span>
            </li>
          );
        })}
      </ul>
      <div className="server-bar__item">
        <button
          type="button"
          className="server-bar__server server-bar__add"
          aria-label={t("bar.add")}
          onClick={() => {
            openDialog({ kind: "add", link: null });
          }}
        >
          <span aria-hidden="true">+</span>
        </button>
        <span className="server-bar__tooltip" aria-hidden="true">
          {t("bar.add")}
        </span>
      </div>
      {menu && menuServer && (
        <ServerOptions
          server={menuServer}
          index={menuIndex}
          count={servers.length}
          connected={statusOf(state, connectedId, menuServer.id) !== null}
          position={{ x: menu.x, y: menu.y }}
          onMove={move}
          onClose={closeMenu}
        />
      )}
    </div>
  );
}
