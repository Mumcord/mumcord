/** Pure channel-tree model: nesting, ordering and the visible rows for keyboard navigation. */
import type { ChannelInfo, User } from "../../api";
import type { ServerState } from "./serverStore";

export interface ChannelNode {
  channel: ChannelInfo;
  children: ChannelNode[];
  /** People whose current channel this is. */
  members: User[];
}

export type RowKind = "channel" | "member";

/** One row of the tree as seen by keyboard navigation. */
export interface TreeRow {
  id: string;
  kind: RowKind;
  /** Row id of the parent channel, `null` for top-level channels. */
  parentId: string | null;
  channelId: number;
  session: number | null;
  expandable: boolean;
  expanded: boolean;
}

export const channelRowId = (channelId: number) => `c${String(channelId)}`;
export const memberRowId = (session: number) => `u${String(session)}`;

const byName = (a: { name: string }, b: { name: string }) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

/** Builds the nested tree. Channel listeners are not part of it; they only show in the member list.
 * Channels whose parent is unknown are shown at the top level. */
export function buildTree(state: Pick<ServerState, "channels" | "users">): ChannelNode[] {
  const nodes = new Map<number, ChannelNode>();
  for (const channel of state.channels.values()) {
    nodes.set(channel.id, { channel, children: [], members: [] });
  }
  for (const user of state.users.values()) {
    nodes.get(user.channelId)?.members.push(user);
  }
  const roots: ChannelNode[] = [];
  for (const node of nodes.values()) {
    const parent = node.channel.parentId === null ? undefined : nodes.get(node.channel.parentId);
    if (parent && parent !== node) parent.children.push(node);
    else roots.push(node);
  }
  const sortNode = (node: ChannelNode) => {
    node.children.sort(
      (a, b) => a.channel.position - b.channel.position || byName(a.channel, b.channel),
    );
    node.members.sort(byName);
    node.children.forEach(sortNode);
  };
  roots.sort((a, b) => a.channel.position - b.channel.position || byName(a.channel, b.channel));
  roots.forEach(sortNode);
  return roots;
}

export function isExpandable(node: ChannelNode): boolean {
  return node.children.length + node.members.length > 0;
}

/** Above this many people an expanded channel shows the avatar stack instead of names (QA.14). */
export const CROWDED_CHANNEL_SIZE = 5;
/** Most avatars shown in a stack before the "+N" chip (QA.14). */
export const STACK_SIZE = 5;

/** Width of one stack avatar, in rem (see AvatarStack.css). */
const STACK_AVATAR_REM = 1.5;
/** Each further avatar overlaps the previous one by 35%, so it adds this much width. */
const STACK_STEP_REM = STACK_AVATAR_REM * 0.65;
/** What the overlapping "+N" chip adds after the last avatar, with room for two digits. */
const STACK_CHIP_REM = 1.25;

const avatarsIn = (rem: number) =>
  rem < STACK_AVATAR_REM ? 0 : 1 + Math.floor((rem - STACK_AVATAR_REM) / STACK_STEP_REM + 1e-9);

/**
 * How many of a channel's `total` people fit in `availableRem` of the stack's line, up to five
 * (QA.29), leaving room for the "+N" chip whenever not everyone fits. At least one is shown.
 */
export function stackFit(availableRem: number, total: number): number {
  const all = Math.min(STACK_SIZE, total, avatarsIn(availableRem));
  const fit = all < total ? Math.min(all, avatarsIn(availableRem - STACK_CHIP_REM)) : all;
  return Math.max(1, fit);
}

const NONE: ReadonlySet<number> = new Set();

/** Whether an expanded channel lists its people by name: not when crowded, unless "Show all" is on. */
export function listsMembers(node: ChannelNode, showAll: ReadonlySet<number>): boolean {
  return node.members.length <= CROWDED_CHANNEL_SIZE || showAll.has(node.channel.id);
}

/**
 * The people shown in a channel's avatar stack: talking people first so they stay visible, then
 * the name-sorted member order, so avatars only move when someone starts or stops talking.
 */
export function stackPeople(
  members: readonly User[],
  talking: ReadonlySet<number>,
  size = STACK_SIZE,
): User[] {
  const speaking: User[] = [];
  const quiet: User[] = [];
  for (const user of members) {
    if (talking.has(user.session)) {
      speaking.push(user);
      if (speaking.length === size) break;
    } else if (quiet.length < size) {
      quiet.push(user);
    }
  }
  return [...speaking, ...quiet].slice(0, size);
}

/**
 * Avatar stacks of the visible channels that show one: collapsed with people, or crowded. Each
 * holds up to five people; the stack itself shows as many as fit its line (`stackFit`).
 */
export function visibleStacks(
  roots: ChannelNode[],
  collapsed: ReadonlySet<number>,
  showAll: ReadonlySet<number>,
  talking: ReadonlySet<number>,
): Map<number, User[]> {
  const stacks = new Map<number, User[]>();
  const walk = (node: ChannelNode) => {
    const expanded = isExpandable(node) && !collapsed.has(node.channel.id);
    if (node.members.length > 0 && (!expanded || !listsMembers(node, showAll))) {
      stacks.set(node.channel.id, stackPeople(node.members, talking));
    }
    if (expanded) for (const child of node.children) walk(child);
  };
  for (const root of roots) walk(root);
  return stacks;
}

/** Rows in display order, skipping the contents of collapsed channels and unlisted crowded ones. */
export function visibleRows(
  roots: ChannelNode[],
  collapsed: ReadonlySet<number>,
  showAll: ReadonlySet<number> = NONE,
): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (node: ChannelNode, parentId: string | null) => {
    const id = channelRowId(node.channel.id);
    const expandable = isExpandable(node);
    const expanded = expandable && !collapsed.has(node.channel.id);
    rows.push({
      id,
      kind: "channel",
      parentId,
      channelId: node.channel.id,
      session: null,
      expandable,
      expanded,
    });
    if (!expanded) return;
    for (const user of listsMembers(node, showAll) ? node.members : []) {
      rows.push({
        id: memberRowId(user.session),
        kind: "member",
        parentId: id,
        channelId: node.channel.id,
        session: user.session,
        expandable: false,
        expanded: false,
      });
    }
    for (const child of node.children) walk(child, id);
  };
  for (const root of roots) walk(root, null);
  return rows;
}
