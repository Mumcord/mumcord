/**
 * ARIA tree of channels with the people in them. One Tab stop (roving tabindex); arrow keys move
 * and expand/collapse. Enter/Space selects a channel's chat or opens a person's profile card,
 * Shift+Enter joins a channel; with the mouse a click selects and a double-click or the "Join"
 * button joins.
 */
import {
  useEffect,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
} from "react";
import { useTranslation } from "react-i18next";
import "./channels.css";
import { announce } from "../../a11y/announcer";
import { useHintPreferences } from "../../a11y/hintPreferences";
import { useListPreferences } from "../../app/preferences";
import { useAvatars } from "../members/useAvatars";
import { rememberCollapsed, rememberedCollapsed } from "./collapseMemory";
import { useDrawSteps } from "./drawSteps";
import { buildSearchIndex, foldQuery, searchTree, type SearchResult } from "./search";
import type { ServerState } from "./serverStore";
import {
  arrangeTree,
  BIG_SERVER_CHANNELS,
  buildTree,
  channelPath,
  channelRowId,
  DRAW_STEP_ROWS,
  initialCollapsed,
  treePrefix,
  visibleRows,
  visibleStacks,
  type ChannelNode,
} from "./tree";
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
  /**
   * Search text (QA.27): the tree shows only matching channels and channels with matching people,
   * opened up to the matches. The remembered expand state stays as it was for when it is cleared.
   */
  filter?: string | undefined;
  ref?: Ref<ChannelTreeHandle> | undefined;
}

export interface ChannelTreeHandle {
  /** Shows the user's voice channel: expands its path, scrolls to it and highlights it (QA.27). */
  jumpToVoice: () => void;
  /** While searching: selects the first matching channel, or focuses the first matching person. */
  openFirstMatch: () => void;
}

/** How long a jumped-to channel stays highlighted. */
const FLASH_MS = 1600;

/**
 * Scrolls the nearest scrollable ancestor so `element` is centred in it. Unlike
 * `scrollIntoView`, it never scrolls the document or clipping ancestors: the app must not move
 * as a whole.
 */
function scrollIntoContainer(element: HTMLElement, smooth: boolean) {
  let container = element.parentElement;
  while (
    container !== null &&
    !["auto", "scroll"].includes(getComputedStyle(container).overflowY)
  ) {
    container = container.parentElement;
  }
  if (container === null) return;
  const box = element.getBoundingClientRect();
  const outer = container.getBoundingClientRect();
  const top = container.scrollTop + box.top - outer.top - (outer.height - box.height) / 2;
  container.scrollTo({ top: Math.max(0, top), behavior: smooth ? "smooth" : "auto" });
}

const prefersReducedMotion = () =>
  typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Highlights a row briefly (CSS `[data-flash]`); returns a function that ends it early. */
function flash(row: HTMLElement): () => void {
  row.setAttribute("data-flash", "");
  const timer = setTimeout(() => {
    row.removeAttribute("data-flash");
  }, FLASH_MS);
  return () => {
    clearTimeout(timer);
    row.removeAttribute("data-flash");
  };
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
  filter = "",
  ref,
}: ChannelTreeProps) {
  const { t } = useTranslation("channels");
  const hintId = useId();
  const { keyboardHints } = useHintPreferences();
  const treeRef = useRef<HTMLDivElement>(null);
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
  // The own channel (voice, or the landing channel while reading along) decides which path a big
  // server opens on first display; the user's later choices win from then on (QA.27).
  const ownChannelId =
    state.selfSession === null ? null : (state.users.get(state.selfSession)?.channelId ?? null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(
    () =>
      rememberedCollapsed(state.serverId) ??
      initialCollapsed(roots, state.channels.size, ownChannelId),
  );
  const [jump, setJump] = useState<{ rowId: string } | null>(null);

  // "Hide empty channels" and "Show talking people first" (FR-2.23). Talking only reorders the
  // tree while that switch is on. Search below always covers every channel.
  const { hideEmptyChannels, talkingFirst } = useListPreferences();
  const talkingOrder = talkingFirst ? state.talking : null;
  const listRoots = useMemo(
    () =>
      arrangeTree(roots, {
        hideEmpty: hideEmptyChannels,
        keep: new Set([ownChannelId, selectedChannelId].filter((id): id is number => id !== null)),
        talkingFirst: talkingOrder,
      }),
    [roots, hideEmptyChannels, ownChannelId, selectedChannelId, talkingOrder],
  );

  // Search (QA.27). Without a query the tree below is the full one, so rows keep their memoised
  // nodes. The index of folded names is built once per tree, only while searching.
  const query = foldQuery(filter);
  const searching = query !== "";
  const index = useMemo(() => (searching ? buildSearchIndex(roots) : null), [roots, searching]);
  const [found, setFound] = useState<{
    roots: ChannelNode[];
    query: string;
    result: SearchResult;
  } | null>(null);
  /** Channels the user collapsed in the search results; dropped when the query changes. */
  const [searchCollapsed, setSearchCollapsed] = useState<ReadonlySet<number>>(new Set());
  let result: SearchResult | null = null;
  if (index === null) {
    if (found !== null) setFound(null);
  } else if (found?.roots === roots && found.query === query) {
    result = found.result;
  } else {
    result = searchTree(roots, index, query, found?.result);
    if (found?.query !== query && searchCollapsed.size > 0) setSearchCollapsed(new Set());
    setFound({ roots, query, result });
  }
  const shownRoots = result?.roots ?? listRoots;
  // Result counts go to the app's polite live region, e.g. "12 channels, 3 people".
  const resultCount =
    result === null
      ? null
      : t("search.count", {
          channels: t("search.channels", { count: result.channels }),
          people: t("search.people", { count: result.people }),
        });
  useEffect(() => {
    if (resultCount !== null) announce(resultCount);
  }, [resultCount, query]);
  const shownCollapsed = result === null ? collapsed : searchCollapsed;

  const rows = useMemo(
    () => visibleRows(shownRoots, shownCollapsed, showAll),
    [shownRoots, shownCollapsed, showAll],
  );
  // A big tree is drawn a few rows per frame: laying out all rows at once froze a slow laptop
  // for longer than 200 ms, both on connecting and on switching to the server (FR-2.24).
  const drawnRows = useDrawSteps(
    rows.length,
    DRAW_STEP_ROWS,
    state.channels.size > BIG_SERVER_CHANNELS,
    String(state.serverId),
  );
  const drawnRoots = useMemo(
    () =>
      drawnRows >= rows.length
        ? shownRoots
        : treePrefix(shownRoots, shownCollapsed, showAll, drawnRows),
    [drawnRows, rows.length, shownRoots, shownCollapsed, showAll],
  );
  const stacks = useMemo(
    () => visibleStacks(shownRoots, shownCollapsed, showAll, state.talking),
    [shownRoots, shownCollapsed, showAll, state.talking],
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
  const selfChannelId = readOnly ? null : ownChannelId;

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

  /** Applies a user's expand/collapse choice and remembers it for the server (QA.27). */
  const changeCollapsed = (update: (current: ReadonlySet<number>) => ReadonlySet<number>) => {
    setCollapsed((current) => {
      const next = update(current);
      rememberCollapsed(state.serverId, next);
      return next;
    });
  };

  const setExpanded = (channelId: number, expanded: boolean) => {
    const update = (current: ReadonlySet<number>) => {
      const next = new Set(current);
      if (expanded) next.delete(channelId);
      else next.add(channelId);
      return next;
    };
    // Expanding or collapsing search results must not change the remembered state (QA.27).
    if (searching) setSearchCollapsed(update);
    else changeCollapsed(update);
  };

  useImperativeHandle(ref, () => ({
    jumpToVoice: () => {
      if (selfChannelId === null) return;
      const path = channelPath(roots, selfChannelId);
      if (path.length === 0) return;
      changeCollapsed((current) =>
        path.some((id) => current.has(id))
          ? new Set([...current].filter((id) => !path.includes(id)))
          : current,
      );
      // A new object each time, so jumping again to the same channel highlights it again.
      setJump({ rowId: channelRowId(selfChannelId) });
    },
    openFirstMatch: () => {
      const byId = new Map(rows.map((row) => [row.id, row]));
      const first = result?.matches.map((id) => byId.get(id)).find((row) => row !== undefined);
      if (first === undefined) return;
      if (first.kind === "channel") onSelect(first.channelId);
      // Focusing a person shows their profile preview (QA.21).
      else focusRow(first.id);
    },
  }));

  // Focus, scroll to and highlight the row once the expanded path is on screen. A frame later:
  // rows pick up the new expand state in a render of their own, after this effect.
  useEffect(() => {
    if (jump === null) return;
    let endFlash: (() => void) | undefined;
    const frame = requestAnimationFrame(() => {
      const row = treeRef.current?.querySelector<HTMLElement>(`[data-row="${jump.rowId}"]`);
      if (!row) return;
      row.focus({ preventScroll: true });
      scrollIntoContainer(row, !prefersReducedMotion());
      endFlash = flash(row);
    });
    return () => {
      cancelAnimationFrame(frame);
      endFlash?.();
    };
  }, [jump]);

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
    collapsed: shownCollapsed,
    showAll,
    tabStopId,
    selectedChannelId,
    selfChannelId,
    selfSession: state.selfSession,
    query,
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
      current.selfSession !== ui.selfSession ||
      current.query !== ui.query
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
          {drawnRoots.map((root) => (
            <ChannelBranch key={channelRowId(root.channel.id)} node={root} level={1} />
          ))}
        </TreeContext.Provider>
      </div>
      {result !== null && result.roots.length === 0 && (
        <p className="channel-tree__empty">{t("search.empty", { query: filter.trim() })}</p>
      )}

      {/* Turned off, the hint only leaves the screen: it still describes the tree to screen readers. */}
      <p id={hintId} className={keyboardHints ? "channel-tree__hint" : "visually-hidden"}>
        {t("tree.hint")}
      </p>
    </div>
  );
}
