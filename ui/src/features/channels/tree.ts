/** Pure channel-tree model: nesting, ordering and the visible rows for keyboard navigation. */
import type { ChannelInfo, User } from "../../api";
import type { ServerState } from "./serverStore";

export interface ChannelNode {
  channel: ChannelInfo;
  children: ChannelNode[];
  /** People whose current channel this is. */
  members: User[];
  /** People listening to this channel from elsewhere (ear icon). */
  listeners: User[];
}

export type RowKind = "channel" | "member" | "listener";

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
export const listenerRowId = (channelId: number, session: number) =>
  `l${String(channelId)}-${String(session)}`;

const byName = (a: { name: string }, b: { name: string }) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

/** Builds the nested tree. Channels whose parent is unknown are shown at the top level. */
export function buildTree(state: Pick<ServerState, "channels" | "users">): ChannelNode[] {
  const nodes = new Map<number, ChannelNode>();
  for (const channel of state.channels.values()) {
    nodes.set(channel.id, { channel, children: [], members: [], listeners: [] });
  }
  for (const user of state.users.values()) {
    nodes.get(user.channelId)?.members.push(user);
    for (const listened of user.listeningChannelIds) {
      // Listening to the channel you are in is redundant: no extra listener row.
      if (listened !== user.channelId) nodes.get(listened)?.listeners.push(user);
    }
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
    node.listeners.sort(byName);
    node.children.forEach(sortNode);
  };
  roots.sort((a, b) => a.channel.position - b.channel.position || byName(a.channel, b.channel));
  roots.forEach(sortNode);
  return roots;
}

export function isExpandable(node: ChannelNode): boolean {
  return node.children.length + node.members.length + node.listeners.length > 0;
}

/** Rows in display order, skipping the contents of collapsed channels. */
export function visibleRows(roots: ChannelNode[], collapsed: ReadonlySet<number>): TreeRow[] {
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
    for (const user of node.members) {
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
    for (const user of node.listeners) {
      rows.push({
        id: listenerRowId(node.channel.id, user.session),
        kind: "listener",
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
