/**
 * ARIA tree of channels with the people in them. One Tab stop (roving tabindex); arrow keys move
 * and expand/collapse. Enter/Space selects a channel's chat or opens a person's profile card,
 * Shift+Enter joins a channel; with the mouse a click selects and a double-click or the "Join"
 * button joins.
 */
import {
  Fragment,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import "./channels.css";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { StatusIcons } from "./PersonStatus";
import { useStatusText } from "./statusText";
import type { ServerState } from "./serverStore";
import {
  buildTree,
  channelRowId,
  isExpandable,
  listenerRowId,
  memberRowId,
  visibleRows,
  type ChannelNode,
} from "./tree";

export interface ChannelTreeProps {
  state: ServerState;
  selectedChannelId: number | null;
  onSelect: (channelId: number) => void;
  onJoin: (channelId: number) => void;
  onOpenPerson: (session: number, opener: HTMLElement | null) => void;
  /** Extra content after a channel's people count, e.g. the unread badge (B22). */
  channelBadge?: ((channelId: number) => ReactNode) | undefined;
}

export function ChannelTree({
  state,
  selectedChannelId,
  onSelect,
  onJoin,
  onOpenPerson,
  channelBadge,
}: ChannelTreeProps) {
  const { t } = useTranslation("channels");
  const statusText = useStatusText();
  const hintId = useId();
  const treeRef = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set());
  const [focusedId, setFocusedId] = useState<string | null>(null);

  const roots = useMemo(() => buildTree(state), [state]);
  const rows = useMemo(() => visibleRows(roots, collapsed), [roots, collapsed]);
  const visibleUsers = useMemo(
    () =>
      rows.flatMap((r) => {
        const user = r.session === null ? undefined : state.users.get(r.session);
        return user === undefined ? [] : [user];
      }),
    [rows, state.users],
  );
  const avatarOf = useAvatars(visibleUsers);
  const selfChannelId =
    state.selfSession === null ? null : (state.users.get(state.selfSession)?.channelId ?? null);

  // The Tab stop falls back to the selected channel when the focused row disappears.
  const tabStopId =
    rows.find((r) => r.id === focusedId)?.id ??
    rows.find((r) => r.kind === "channel" && r.channelId === selectedChannelId)?.id ??
    rows[0]?.id;

  const focusRow = (id: string | undefined) => {
    if (id === undefined) return;
    setFocusedId(id);
    treeRef.current?.querySelector<HTMLElement>(`[data-row="${id}"]`)?.focus();
  };

  const setExpanded = (channelId: number, expanded: boolean) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (expanded) next.delete(channelId);
      else next.add(channelId);
      return next;
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // The row that has focus (the event may come from a nested element such as the Join button).
    const focused =
      event.target instanceof HTMLElement
        ? event.target.closest<HTMLElement>("[data-row]")?.dataset.row
        : undefined;
    const currentId = focused ?? tabStopId;
    const index = rows.findIndex((r) => r.id === currentId);
    const row = rows[index];
    if (!row) return;
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        focusRow(rows[index + 1]?.id);
        break;
      case "ArrowUp":
        focusRow(rows[index - 1]?.id);
        break;
      case "Home":
        focusRow(rows[0]?.id);
        break;
      case "End":
        focusRow(rows[rows.length - 1]?.id);
        break;
      case "ArrowRight":
        if (row.expandable && !row.expanded) setExpanded(row.channelId, true);
        else if (row.expanded) focusRow(rows[index + 1]?.id);
        break;
      case "ArrowLeft":
        if (row.expanded) setExpanded(row.channelId, false);
        else focusRow(row.parentId ?? undefined);
        break;
      case "Enter":
      case " ":
        if (row.kind === "channel") {
          if (event.key === "Enter" && event.shiftKey) onJoin(row.channelId);
          else onSelect(row.channelId);
        } else if (row.session !== null) {
          const opener = treeRef.current?.querySelector<HTMLElement>(`[data-row="${row.id}"]`);
          onOpenPerson(row.session, opener ?? null);
        }
        break;
      default:
        handled = false;
    }
    if (handled) {
      event.preventDefault();
      event.stopPropagation();
    }
  };

  // click(detail 2) and dblclick both fire for one gesture; join only once.
  const lastJoin = useRef({ id: -1, at: 0 });
  const joinOnce = (channelId: number, now: number) => {
    if (lastJoin.current.id === channelId && now - lastJoin.current.at < 600) return;
    lastJoin.current = { id: channelId, at: now };
    onJoin(channelId);
  };

  const personItem = (user: User, rowId: string, level: number, listener: boolean) => {
    const talking = !listener && state.talking.has(user.session);
    const labelId = `${hintId}-${rowId}`;
    const status = statusText(user, talking, listener);
    return (
      // Keyboard input is handled once on the tree (roving focus), not per item.
      // eslint-disable-next-line jsx-a11y/click-events-have-key-events
      <div
        key={rowId}
        role="treeitem"
        aria-level={level}
        aria-selected={false}
        aria-labelledby={labelId}
        tabIndex={tabStopId === rowId ? 0 : -1}
        data-row={rowId}
        className="tree-person"
        data-talking={talking}
        data-listener={listener}
        onFocus={(event) => {
          if (event.target === event.currentTarget) setFocusedId(rowId);
        }}
        onClick={(event) => {
          event.stopPropagation();
          setFocusedId(rowId);
          onOpenPerson(user.session, event.currentTarget);
        }}
      >
        <span className="tree-person__row">
          <Avatar name={user.name} src={avatarOf(user)} talking={talking} size="sm" />
          <span id={labelId} className="tree-person__name">
            {user.name}
            {user.session === state.selfSession && (
              <span className="tree-person__you"> {t("tree.you")}</span>
            )}
            {status.length > 0 && <span className="visually-hidden">, {status.join(", ")}</span>}
          </span>
          <StatusIcons user={user} listener={listener} />
        </span>
      </div>
    );
  };

  const channelItem = (node: ChannelNode, level: number): ReactNode => {
    const { channel } = node;
    const rowId = channelRowId(channel.id);
    const labelId = `${hintId}-${rowId}`;
    const expandable = isExpandable(node);
    const expanded = expandable && !collapsed.has(channel.id);
    const count = node.members.length;
    const isOwn = channel.id === selfChannelId;
    const groupId = `${hintId}-group-${rowId}`;
    // The group is a DOM sibling (linked via aria-owns) so the treeitem's visible
    // content is only its own row; WCAG 2.5.3 label-in-name then matches its name.
    return (
      <Fragment key={rowId}>
        {/* Keyboard input is handled once on the tree (roving focus), not per item. */}
        {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events */}
        <div
          role="treeitem"
          aria-level={level}
          aria-owns={expanded ? groupId : undefined}
          aria-selected={channel.id === selectedChannelId}
          aria-expanded={expandable ? expanded : undefined}
          aria-labelledby={labelId}
          aria-keyshortcuts="Shift+Enter"
          tabIndex={tabStopId === rowId ? 0 : -1}
          data-row={rowId}
          className="tree-channel"
          onFocus={(event) => {
            if (event.target === event.currentTarget) setFocusedId(rowId);
          }}
          onClick={(event) => {
            event.stopPropagation();
            setFocusedId(rowId);
            onSelect(channel.id);
            // Real mice: the second click (detail >= 2) joins even if the native dblclick lands elsewhere.
            if (event.detail >= 2) joinOnce(channel.id, event.timeStamp);
          }}
          onDoubleClick={(event) => {
            event.stopPropagation();
            joinOnce(channel.id, event.timeStamp);
          }}
        >
          <div className="tree-channel__row" data-selected={channel.id === selectedChannelId}>
            <span
              className="tree-channel__toggle"
              data-expandable={expandable}
              aria-hidden="true"
              onClick={(event) => {
                if (!expandable) return;
                event.stopPropagation();
                setExpanded(channel.id, !expanded);
              }}
              onDoubleClick={(event) => {
                event.stopPropagation();
              }}
            >
              {expanded ? "▾" : "▸"}
            </span>
            <span id={labelId} className="tree-channel__label">
              <span className="tree-channel__name">{channel.name}</span>
              {isOwn && <span className="visually-hidden">, {t("tree.yourChannel")}</span>}
              {count > 0 && (
                // The digits are CSS-generated (::before) so the row's visible text stays a run of
                // its accessible name (WCAG 2.5.3); the spoken form is the visually hidden phrase.
                <span className="tree-channel__count" data-count={count}>
                  <span className="visually-hidden">, {t("tree.people", { count })}</span>
                </span>
              )}
              {channelBadge?.(channel.id)}
            </span>
            {!isOwn && channel.permissions.enter && (
              <button
                type="button"
                className="tree-channel__join"
                // The label is CSS-generated (::before): the button shows only on hover/focus and
                // its text must not join the treeitem's visible text (WCAG 2.5.3 label-in-name).
                data-label={t("tree.join")}
                tabIndex={-1}
                aria-hidden="true"
                onClick={(event) => {
                  event.stopPropagation();
                  onJoin(channel.id);
                }}
                onDoubleClick={(event) => {
                  event.stopPropagation();
                }}
              />
            )}
          </div>
        </div>
        {expanded && (
          <div role="group" id={groupId}>
            {node.members.map((u) => personItem(u, memberRowId(u.session), level + 1, false))}
            {node.listeners.map((u) =>
              personItem(u, listenerRowId(channel.id, u.session), level + 1, true),
            )}
            {node.children.map((child) => channelItem(child, level + 1))}
          </div>
        )}
      </Fragment>
    );
  };

  return (
    <div className="channel-tree">
      {/* Focus lives on the items (roving tabindex), as in the WAI-ARIA tree pattern. */}
      {/* eslint-disable-next-line jsx-a11y/interactive-supports-focus */}
      <div
        ref={treeRef}
        role="tree"
        aria-label={t("tree.label")}
        aria-describedby={hintId}
        onKeyDown={onKeyDown}
      >
        {roots.map((root) => channelItem(root, 1))}
      </div>
      <p id={hintId} className="channel-tree__hint">
        {t("tree.hint")}
      </p>
    </div>
  );
}
