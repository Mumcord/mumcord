/**
 * Rows of the channel tree as memoised components (scale report, P3). A row re-renders only when
 * its own data changes: its tree node (which keeps its identity while unchanged, see `buildTree`),
 * or one value it selects from the tree's server state (talking), the tree's UI state (expanded, selected,
 * Tab stop) or the loaded avatars. With 1000 people, one person moving or talking then redraws a
 * few rows instead of all of them.
 */
import { memo, useContext, useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { Avatar } from "../members/Avatar";
import { ProfilePreviewTrigger } from "../members/ProfilePreview";
import { previewOnFocus, releasePreview } from "../members/profilePreviewStore";
import { AvatarStack, type StackPerson } from "./AvatarStack";
import { StatusIcons } from "./PersonStatus";
import type { ServerState } from "./serverStore";
import { useStatusText } from "./statusText";
import { cachedSelector, sameValue } from "./cachedSelector";
import { TreeContext, type TreeContextValue, type ValueStore } from "./treeContext";
import {
  channelRowId,
  CROWDED_CHANNEL_SIZE,
  isExpandable,
  memberRowId,
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
  const { t } = useTranslation("channels");
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
  const count = node.members.length;
  const crowded = expanded && count > CROWDED_CHANNEL_SIZE;
  const hasStack = count > 0 && (!expanded || (count > CROWDED_CHANNEL_SIZE && !listingAll));
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
        className="tree-channel"
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
      >
        <div className="tree-channel__row" data-selected={selected} data-voice={isOwn}>
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
            {expanded ? "▾" : "▸"}
          </span>
          <span id={labelId} className="tree-channel__label">
            <span className="tree-channel__name">{channel.name}</span>
            {isOwn && <span className="visually-hidden">, {t("tree.voiceHere")}</span>}
            {count > 0 && (
              // The digits are CSS-generated (::before) so the row's visible text stays a run of
              // its accessible name (WCAG 2.5.3); the spoken form is the visually hidden phrase.
              <span className="tree-channel__count" data-count={count}>
                <span className="visually-hidden">, {t("tree.people", { count })}</span>
              </span>
            )}
            {actions.current.channelBadge?.(id)}
            {/* The stack's label follows in the name (aria-labelledby); this is its comma. */}
            {showStack && <span className="visually-hidden">,</span>}
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
        <div role="group" id={groupId}>
          {(!crowded || listingAll) &&
            node.members.map((user) => (
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
  const { t } = useTranslation("channels");
  const statusText = useStatusText();
  const tree = useTree();
  const session = user.session;
  const rowId = memberRowId(session);
  const labelId = `${tree.hintId}-${rowId}`;
  const talking = useSelect(tree.server, (s: ServerState) => s.talking.has(session));
  const tabbable = useSelect(tree.ui, (ui) => ui.tabStopId === rowId);
  const isSelf = useSelect(tree.ui, (ui) => ui.selfSession === session);
  const src = useSelect(tree.avatars, (avatarOf) => avatarOf(user));
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
      onFocus={(event) => {
        if (event.target !== event.currentTarget) return;
        actions.current.focus(rowId);
        previewOnFocus(session, event.currentTarget);
      }}
      onBlur={releasePreview}
      onClick={(event) => {
        event.stopPropagation();
        actions.current.focus(rowId);
        actions.current.openPerson(session, event.currentTarget);
      }}
    >
      <span className="tree-person__row">
        <ProfilePreviewTrigger session={session}>
          <Avatar name={user.name} src={src} talking={talking} size="sm" />
          <span id={labelId} className="tree-person__name" data-local-muted={user.localMute}>
            {user.name}
            {isSelf && <span className="tree-person__you"> {t("tree.you")}</span>}
            {status.length > 0 && <span className="visually-hidden">, {status.join(", ")}</span>}
          </span>
        </ProfilePreviewTrigger>
        <StatusIcons user={user} />
      </span>
    </div>
  );
});
