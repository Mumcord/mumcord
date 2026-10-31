/**
 * Rows of the channel tree as memoised components (scale report, P3). A row re-renders only when
 * its own data changes: its tree node (which keeps its identity while unchanged, see `buildTree`),
 * or one value it selects from the tree's server state (talking), the tree's UI state (expanded, selected,
 * Tab stop) or the loaded avatars. With 1000 people, one person moving or talking then redraws a
 * few rows instead of all of them.
 */
import {
  memo,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type DragEvent,
  type ReactNode,
} from "react";
import { useT } from "../../i18n/useT";
import type { User } from "../../api";
import { ProfilePreviewTrigger } from "../members/ProfilePreview";
import { openPersonMenuFromEvent } from "../members/personMenuStore";
import { clickPerson, doubleClickPerson } from "../members/personClick";
import { previewOnFocus, releasePreview } from "../members/profilePreviewStore";
import { AvatarStack, type StackPerson } from "./AvatarStack";
import { StatusIcons } from "./PersonStatus";
import type { ServerState } from "./serverStore";
import { useStatusText } from "./statusText";
import { cachedSelector, sameValue } from "./cachedSelector";
import { foldName, matchRange } from "./search";
import { TreeContext, type TreeContextValue, type ValueStore } from "./treeContext";
import { MumcordBadge } from "./MumcordBadge";
import { BlockedBadge } from "../blocking/BlockedBadge";
import { LiveAvatar, LiveBadge } from "../screenshare/LiveBadge";
import {
  channelRowId,
  CROWDED_CHANNEL_SIZE,
  indentRem,
  isExpandable,
  memberRowId,
  shownMembers,
  stackPeople,
  type ChannelNode,
} from "./tree";

function useTree(): TreeContextValue {
  const tree = useContext(TreeContext);
  if (tree === null) throw new Error("tree rows must be rendered inside ChannelTree");
  return tree;
}

/** Selected value of a store; re-renders only when it changes (see `sameValue`). */
function useSelect<S, T>(store: ValueStore<S>, select: (state: S) => T): T {
  const getSnapshot = useMemo(() => cachedSelector(store.get, select, sameValue), [store, select]);
  return useSyncExternalStore(store.subscribe, getSnapshot);
}

/** Reports the element's people to the tree's on-screen tracker while mounted. */
function useWatchOnScreen(tree: TreeContextValue, sessions: readonly number[]) {
  const ref = useRef<HTMLDivElement>(null);
  const key = sessions.join(",");
  useEffect(() => {
    const element = ref.current;
    if (element === null || key === "") return;
    return tree.onScreen.watch(element, key.split(",").map(Number));
  }, [tree, key]);
  return ref;
}

/** `text` with the part matching the search `query` (folded) marked (QA.27). */
function Highlighted({ text, query }: { text: string; query: string }) {
  const range = useMemo(
    () => (query === "" ? null : matchRange(foldName(text), query)),
    [text, query],
  );
  if (range === null) return text;
  return (
    <>
      {text.slice(0, range.start)}
      <mark className="tree-match">{text.slice(range.start, range.end)}</mark>
      {text.slice(range.end)}
    </>
  );
}

/** Drag data type of a person row (issue #172); its value is the session. */
const PERSON_DRAG_TYPE = "application/x-mumcord-person";

/** Small status icon after a channel name, with its meaning for screen readers. */
const GLYPHS = {
  // Speaker: the channel the user is in for voice.
  voice: <path d="M11 5 6 9H2v6h4l5 4zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14" />,
  // Clock: a temporary channel, removed by the server once empty.
  temporary: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  text: <path d="M5 9h14M5 15h14M10 3 8 21M16 3l-2 18" />,
};

/**
 * Glyph before every channel name, always in the same place (#172): 🔊 for the own voice channel,
 * ⏱ for temporary channels and # for all others. Decorative: the row's name says the rest.
 */
function ChannelGlyph({ kind }: { kind: keyof typeof GLYPHS }) {
  return (
    <svg
      className="tree-channel__glyph"
      data-kind={kind}
      viewBox="0 0 24 24"
      width="14"
      height="14"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {GLYPHS[kind]}
    </svg>
  );
}

function StatusIcon({
  label,
  kind,
  children,
}: {
  label: string;
  kind: string;
  children: ReactNode;
}) {
  return (
    <span className={`tree-channel__status tree-channel__status--${kind}`}>
      <svg
        viewBox="0 0 24 24"
        width="12"
        height="12"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        {children}
      </svg>
      <span className="visually-hidden">, {label}</span>
    </span>
  );
}

const NO_STACK: readonly User[] = [];
const NO_SESSIONS: readonly number[] = [];

/** One channel row with its people and sub-channels (when expanded). */
// The inner function has another name: inside a named function expression its own name would
// refer to the unmemoised function, and nested branches would always re-render.
export const ChannelBranch = memo(function ChannelBranchRow({
  node,
  level,
}: {
  node: ChannelNode;
  level: number;
}) {
  const t = useT("channels");
  const tree = useTree();
  const { channel } = node;
  const id = channel.id;
  const rowId = channelRowId(id);
  const labelId = `${tree.hintId}-${rowId}`;
  const stackId = `${tree.hintId}-stack-${rowId}`;
  const groupId = `${tree.hintId}-group-${rowId}`;
  const expandable = isExpandable(node);
  const expanded = useSelect(tree.ui, (ui) => expandable && !ui.collapsed.has(id));
  const listingAll = useSelect(tree.ui, (ui) => ui.showAll.has(id));
  const selected = useSelect(tree.ui, (ui) => ui.selectedChannelId === id);
  const tabbable = useSelect(tree.ui, (ui) => ui.tabStopId === rowId);
  const isOwn = useSelect(tree.ui, (ui) => ui.selfChannelId === id);
  const query = useSelect(tree.ui, (ui) => ui.query);
  const count = node.members.length;
  // While searching, a channel lists only its matching people, never its stack (QA.27).
  const shown = shownMembers(node);
  const crowded = expanded && count > CROWDED_CHANNEL_SIZE && shown === null;
  const hasStack = count > 0 && (!expanded || (crowded && !listingAll));
  const stack = useSelect(tree.server, (s: ServerState) =>
    hasStack ? stackPeople(node.members, s.talking) : NO_STACK,
  );
  const stackTalking = useSelect(tree.server, (s: ServerState) =>
    stack.map((user) => s.talking.has(user.session)),
  );
  const stackSrc = useSelect(tree.avatars, (avatarOf) => stack.map((user) => avatarOf(user)));
  const rowRef = useWatchOnScreen(
    tree,
    stack.filter((user) => user.hasAvatar).map((user) => user.session),
  );
  const people = useMemo<StackPerson[]>(
    () =>
      stack.map((user, i) => ({
        user,
        talking: stackTalking[i] ?? false,
        src: stackSrc[i] ?? null,
      })),
    [stack, stackTalking, stackSrc],
  );
  const showStack = stack.length > 0;
  const showAllLabel = listingAll ? t("tree.showFewer") : t("tree.showAll", { total: count });
  const actions = tree.actions;
  // User limit (FR-3.85): `(n/max)` after the name; full channels are dimmed.
  const max = channel.maxUsers;
  const full = max !== null && count >= max;
  const places = max === null ? "" : t("tree.places", { count: max, taken: count });
  const [dropping, setDropping] = useState(false);
  const acceptsDrop = (event: DragEvent) =>
    channel.permissions.moveUsers && event.dataTransfer.types.includes(PERSON_DRAG_TYPE);
  // Indent of the channel's people and sub-channels; also read by its avatar stack line, which is
  // indented like its people (QA.27).
  const indent = { "--tree-indent": `${String(indentRem(level + 1))}rem` } as CSSProperties;
  // The group is a DOM sibling (linked via aria-owns) so the treeitem's visible
  // content is only its own row; WCAG 2.5.3 label-in-name then matches its name.
  return (
    <>
      {/* Keyboard input is handled once on the tree (roving focus), not per item. */}
      {/* eslint-disable-next-line jsx-a11y/click-events-have-key-events */}
      <div
        ref={rowRef}
        role="treeitem"
        aria-level={level}
        aria-owns={expanded ? groupId : undefined}
        aria-selected={selected}
        aria-expanded={expandable ? expanded : undefined}
        aria-labelledby={showStack ? `${labelId} ${stackId}` : labelId}
        aria-keyshortcuts="Shift+Enter"
        tabIndex={tabbable ? 0 : -1}
        data-row={rowId}
        data-full={full || undefined}
        data-tooltip={full ? t("tree.full") : undefined}
        data-tooltip-detail={full ? places : undefined}
        data-tooltip-placement={full ? "right" : undefined}
        className="tree-channel"
        style={indent}
        onFocus={(event) => {
          if (event.target === event.currentTarget) actions.current.focus(rowId);
        }}
        onClick={(event) => {
          event.stopPropagation();
          actions.current.focus(rowId);
          actions.current.select(id);
          // Real mice: the second click (detail >= 2) joins even if the native dblclick lands elsewhere.
          if (event.detail >= 2) actions.current.joinOnce(id, event.timeStamp);
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
          actions.current.joinOnce(id, event.timeStamp);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          actions.current.focus(rowId);
          const keyboard = event.clientX === 0 && event.clientY === 0;
          const rect = event.currentTarget.getBoundingClientRect();
          actions.current.openMenu(id, {
            x: keyboard ? rect.left : event.clientX,
            y: keyboard ? rect.bottom : event.clientY,
          });
        }}
        onDragOver={(event) => {
          if (!acceptsDrop(event)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          if (!dropping) setDropping(true);
        }}
        onDragLeave={(event) => {
          const to = event.relatedTarget;
          if (!(to instanceof Node && event.currentTarget.contains(to))) setDropping(false);
        }}
        onDrop={(event) => {
          setDropping(false);
          if (!acceptsDrop(event)) return;
          event.preventDefault();
          const session = Number(event.dataTransfer.getData(PERSON_DRAG_TYPE));
          if (Number.isInteger(session)) actions.current.movePerson(session, id);
        }}
      >
        <div
          className="tree-channel__row"
          data-selected={selected}
          data-voice={isOwn}
          data-drop={dropping || undefined}
        >
          <span
            className="tree-channel__toggle"
            data-expandable={expandable}
            aria-hidden="true"
            onClick={(event) => {
              if (!expandable) return;
              event.stopPropagation();
              actions.current.setExpanded(id, !expanded);
            }}
            onDoubleClick={(event) => {
              event.stopPropagation();
            }}
          >
            {/* Thin chevron like the mockup; it turns down while the channel is expanded. */}
            <svg
              className="tree-channel__chevron"
              data-expanded={expanded}
              viewBox="0 0 24 24"
              width="14"
              height="14"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              focusable="false"
            >
              <path d="m9 6 6 6-6 6" />
            </svg>
          </span>
          <ChannelGlyph kind={isOwn ? "voice" : channel.temporary ? "temporary" : "text"} />
          <span id={labelId} className="tree-channel__label">
            <span className="tree-channel__name" data-tooltip={channel.name} data-tooltip-overflow>
              <Highlighted text={channel.name} query={query} />
            </span>
            {max !== null && (
              // Generated text like the count below (WCAG 2.5.3); the spoken form follows it.
              <span
                className="tree-channel__limit"
                data-limit={`(${String(count)}/${String(max)})`}
                data-full={full}
              >
                <span className="visually-hidden">
                  , {full ? t("tree.fullPlaces", { places }) : places}
                </span>
              </span>
            )}
            {!channel.permissions.enter && (
              // The server's ACL refuses entering; joining asks for a channel password (FR-2.78).
              <span className="tree-channel__lock">
                <svg
                  viewBox="0 0 24 24"
                  width="12"
                  height="12"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                  focusable="false"
                >
                  <path d="M6 11h12v10H6zM8 11V7a4 4 0 0 1 8 0v4" />
                </svg>
                <span className="visually-hidden">, {t("tree.locked")}</span>
              </span>
            )}
            {channel.temporary && (
              // Shown by the ⏱ glyph before the name; here only for screen readers.
              <span className="visually-hidden">, {t("tree.temporary")}</span>
            )}
            {channel.links.length > 0 && (
              <StatusIcon kind="linked" label={t("tree.linked")}>
                <path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" />
                <path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" />
              </StatusIcon>
            )}
            {count > 0 && max === null && (
              // The digits are CSS-generated (::before) so the row's visible text stays a run of
              // its accessible name (WCAG 2.5.3); the spoken form is the visually hidden phrase.
              <span className="tree-channel__count" data-count={count}>
                <span className="visually-hidden">, {t("tree.people", { count })}</span>
              </span>
            )}
            {isOwn && (
              // Generated like the counts (WCAG 2.5.3): the visible "you are here" follows the
              // count, so as text it would break the name's order; the spoken form is hidden text.
              <span className="tree-channel__here" data-label={t("tree.youAreHere")}>
                <span className="visually-hidden">, {t("tree.youAreHere")}</span>
              </span>
            )}
            {actions.current.channelBadge?.(id)}
            {/* The stack's label follows in the name (aria-labelledby); this is its comma. */}
            {showStack && <span className="visually-hidden">,</span>}
          </span>
          {/* Also on locked channels: joining one asks for its password (FR-2.78). */}
          {!isOwn && (
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
                actions.current.join(id);
              }}
              onDoubleClick={(event) => {
                event.stopPropagation();
              }}
            />
          )}
          {actions.current.channelAction && (
            // Fixed-width slot at the row's end: the hover-only Join button appears to its left,
            // so the action never moves under the pointer (moving targets caused misclicks).
            <span className="tree-channel__action">
              {actions.current.channelAction({ channelId: id, name: channel.name, tabbable })}
            </span>
          )}
        </div>
        {(showStack || crowded) && (
          // Own line under the row, so the name keeps the row's width (QA.14, QA.29). Inside the
          // treeitem (a tree group may only hold treeitems), so the toggle's text is
          // CSS-generated like the Join button's (WCAG 2.5.3 label-in-name).
          <div className="avatar-stack__line">
            {showStack && <AvatarStack id={stackId} people={people} total={count} />}
            {crowded && (
              <button
                type="button"
                className="avatar-stack__toggle"
                data-label={showAllLabel}
                aria-label={showAllLabel}
                aria-expanded={listingAll}
                tabIndex={tabbable ? 0 : -1}
                onClick={(event) => {
                  event.stopPropagation();
                  actions.current.toggleShowAll(id);
                }}
                onDoubleClick={(event) => {
                  event.stopPropagation();
                }}
                onKeyDown={(event) => {
                  // The tree selects the channel on Enter/Space; the toggle must not.
                  if (event.key === "Enter" || event.key === " ") event.stopPropagation();
                }}
              />
            )}
          </div>
        )}
      </div>
      {expanded && (
        <div role="group" id={groupId} style={indent}>
          {(shown ?? (!crowded || listingAll ? node.members : NO_STACK)).map((user) => (
            <PersonRow key={memberRowId(user.session)} user={user} level={level + 1} />
          ))}
          {node.children.map((child) => (
            <ChannelBranch key={channelRowId(child.channel.id)} node={child} level={level + 1} />
          ))}
        </div>
      )}
    </>
  );
});

/** One person in the tree. */
const PersonRow = memo(function PersonTreeRow({ user, level }: { user: User; level: number }) {
  const t = useT("channels");
  const statusText = useStatusText();
  const tree = useTree();
  const session = user.session;
  const rowId = memberRowId(session);
  const labelId = `${tree.hintId}-${rowId}`;
  const talking = useSelect(tree.server, (s: ServerState) => s.talking.has(session));
  const tabbable = useSelect(tree.ui, (ui) => ui.tabStopId === rowId);
  const isSelf = useSelect(tree.ui, (ui) => ui.selfSession === session);
  const query = useSelect(tree.ui, (ui) => ui.query);
  const src = useSelect(tree.avatars, (avatarOf) => avatarOf(user));
  // Dragging a person onto a channel moves them; offered only with the move permission in
  // their channel and never for oneself (joining is how one moves oneself, issue #172).
  const movable = useSelect(
    tree.server,
    (s: ServerState) => s.channels.get(user.channelId)?.permissions.moveUsers === true,
  );
  const draggable = movable && !isSelf;
  const rowRef = useWatchOnScreen(tree, user.hasAvatar ? [session] : NO_SESSIONS);
  const status = statusText(user, talking);
  const actions = tree.actions;
  return (
    // Keyboard input is handled once on the tree (roving focus), not per item.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events
    <div
      ref={rowRef}
      role="treeitem"
      aria-level={level}
      aria-selected={false}
      aria-labelledby={labelId}
      tabIndex={tabbable ? 0 : -1}
      data-row={rowId}
      className="tree-person"
      data-talking={talking}
      draggable={draggable || undefined}
      onDragStart={
        draggable
          ? (event) => {
              event.dataTransfer.setData(PERSON_DRAG_TYPE, String(session));
              event.dataTransfer.effectAllowed = "move";
            }
          : undefined
      }
      onFocus={(event) => {
        if (event.target !== event.currentTarget) return;
        actions.current.focus(rowId);
        previewOnFocus(session, event.currentTarget);
      }}
      onBlur={releasePreview}
      onContextMenu={(event) => {
        openPersonMenuFromEvent(session, event);
      }}
      onClick={(event) => {
        event.stopPropagation();
        actions.current.focus(rowId);
        clickPerson(event.detail, session, event.currentTarget);
      }}
      onDoubleClick={(event) => {
        event.stopPropagation();
        doubleClickPerson(session);
      }}
    >
      <span className="tree-person__row">
        <ProfilePreviewTrigger session={session}>
          <LiveAvatar session={session} name={user.name} src={src} talking={talking} size="sm" />
          <span id={labelId} className="tree-person__name" data-local-muted={user.localMute}>
            <Highlighted text={user.name} query={query} />
            {isSelf && <span className="tree-person__you"> {t("tree.you")}</span>}
            <MumcordBadge user={user} />
            {!isSelf && <BlockedBadge user={user} />}
            <LiveBadge session={session} />
            {status.length > 0 && <span className="visually-hidden">, {status.join(", ")}</span>}
          </span>
        </ProfilePreviewTrigger>
        <StatusIcons user={user} />
      </span>
    </div>
  );
});
