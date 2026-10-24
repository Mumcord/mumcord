/**
 * ARIA tree of channels with the people in them. One Tab stop (roving tabindex); arrow keys move
 * and expand/collapse. Enter/Space selects a channel's chat or opens a person's profile card,
 * Shift+Enter joins a channel; with the mouse a click selects and a double-click or the "Join"
 * button joins.
 */
import {
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useTranslation } from "react-i18next";
import "./channels.css";
import { useAvatars } from "../members/useAvatars";
import type { ServerState } from "./serverStore";
import { buildTree, channelRowId, visibleRows, visibleStacks, type ChannelNode } from "./tree";
import {
  onScreenTracker,
  TreeContext,
  valueStore,
  type TreeActions,
  type TreeContextValue,
  type TreeUi,
} from "./treeContext";
import { ChannelBranch } from "./treeRows";

export interface ChannelTreeProps {
  state: ServerState;
  /** Reading along (not joined for voice): no channel is highlighted as the voice channel. */
  readOnly?: boolean;
  selectedChannelId: number | null;
  onSelect: (channelId: number) => void;
  onJoin: (channelId: number) => void;
  onOpenPerson: (session: number, opener: HTMLElement | null) => void;
  /**
   * Extra content after a channel's people count, e.g. the unread badge (B22). Rows call it only
   * when they re-render, so it must return a component that keeps itself up to date.
   */
  channelBadge?: ((channelId: number) => ReactNode) | undefined;
  /**
   * A button in a fixed slot of each channel row, e.g. the follow star (QA.13). `tabbable` is true
   * for the row holding the tree's Tab stop, so Tab reaches the button from that row. Like
   * `channelBadge`, it must return a self-updating component.
   */
  channelAction?: ((channel: ChannelAction) => ReactNode) | undefined;
}

export interface ChannelAction {
  channelId: number;
  name: string;
  tabbable: boolean;
}

export function ChannelTree({
  state,
  readOnly = false,
  selectedChannelId,
  onSelect,
  onJoin,
  onOpenPerson,
  channelBadge,
  channelAction,
}: ChannelTreeProps) {
  const { t } = useTranslation("channels");
  const hintId = useId();
  const treeRef = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set());
  const [focusedId, setFocusedId] = useState<string | null>(null);
  /** Crowded channels whose people are listed by name for this session ("Show all", QA.14). */
  const [showAll, setShowAll] = useState<ReadonlySet<number>>(new Set());

  // Only channels and people shape the tree; talking changes must not rebuild it (scale report).
  // The previous tree lets unchanged channels keep their nodes, so their rows skip re-rendering.
  const [tree, setTree] = useState<{ source: ServerState | null; roots: ChannelNode[] }>({
    source: null,
    roots: [],
  });
  let roots = tree.roots;
  if (
    tree.source === null ||
    tree.source.channels !== state.channels ||
    tree.source.users !== state.users
  ) {
    roots = buildTree(state, tree.roots);
    setTree({ source: state, roots });
  }
  const rows = useMemo(() => visibleRows(roots, collapsed, showAll), [roots, collapsed, showAll]);
  const stacks = useMemo(
    () => visibleStacks(roots, collapsed, showAll, state.talking),
    [roots, collapsed, showAll, state.talking],
  );
  // Stable for the tree's lifetime (see `context` below); created first because avatars need it.
  const [onScreen] = useState(onScreenTracker);
  const seen = useSyncExternalStore(onScreen.subscribe, onScreen.get);
  // Avatars load only for people whose rows came near the screen (scale report, P5).
  const visibleUsers = useMemo(
    () =>
      [
        ...rows.flatMap((r) => {
          const user = r.session === null ? undefined : state.users.get(r.session);
          return user === undefined ? [] : [user];
        }),
        ...[...stacks.values()].flat(),
      ].filter((user) => seen.has(user.session)),
    [rows, stacks, state.users, seen],
  );
  const avatarOf = useAvatars(visibleUsers);
  // The voice channel: none while reading along, since no one is in a call yet.
  const selfChannelId =
    readOnly || state.selfSession === null
      ? null
      : (state.users.get(state.selfSession)?.channelId ?? null);

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

  const toggleShowAll = (channelId: number) => {
    setShowAll((current) => {
      const next = new Set(current);
      if (!next.delete(channelId)) next.add(channelId);
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
  const actions: TreeActions = {
    focus: setFocusedId,
    setExpanded,
    toggleShowAll,
    select: onSelect,
    join: onJoin,
    joinOnce: (channelId, now) => {
      if (lastJoin.current.id === channelId && now - lastJoin.current.at < 600) return;
      lastJoin.current = { id: channelId, at: now };
      onJoin(channelId);
    },
    openPerson: onOpenPerson,
    channelBadge,
    channelAction,
  };
  const ui: TreeUi = {
    collapsed,
    showAll,
    tabStopId,
    selectedChannelId,
    selfChannelId,
    selfSession: state.selfSession,
  };
  // Stable for the tree's lifetime: rows read the changing parts through these stores, so a
  // change re-renders only the rows it concerns (see `treeRows`).
  const actionsRef = useRef(actions);
  const [context] = useState<TreeContextValue>(() => ({
    hintId,
    server: valueStore(state),
    ui: valueStore(ui),
    avatars: valueStore(avatarOf),
    onScreen,
    actions: actionsRef,
  }));
  useLayoutEffect(() => {
    actionsRef.current = actions;
    const current = context.ui.get();
    if (
      current.collapsed !== ui.collapsed ||
      current.showAll !== ui.showAll ||
      current.tabStopId !== ui.tabStopId ||
      current.selectedChannelId !== ui.selectedChannelId ||
      current.selfChannelId !== ui.selfChannelId ||
      current.selfSession !== ui.selfSession
    ) {
      context.ui.set(ui);
    }
    context.avatars.set(avatarOf);
    context.server.set(state);
  });

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
        <TreeContext.Provider value={context}>
          {roots.map((root) => (
            <ChannelBranch key={channelRowId(root.channel.id)} node={root} level={1} />
          ))}
        </TreeContext.Provider>
      </div>
      <p id={hintId} className="channel-tree__hint">
        {t("tree.hint")}
      </p>
    </div>
  );
}
