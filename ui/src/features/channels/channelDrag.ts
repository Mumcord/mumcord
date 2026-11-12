/**
 * Dragging channels in the tree (issue #676): where a drop lands and which channel edits it takes.
 * Dropping onto the middle of a row moves the channel under it; the top or bottom quarter places
 * it above or below that row among its siblings. The server has the final say on every edit.
 */
import type { ChannelInfo } from "../../api";
import { compareChannels } from "./tree";
import { valueStore } from "./treeContext";

/** Data type of a dragged channel; people use their own (`PERSON_DRAG_TYPE`). */
export const CHANNEL_DRAG_TYPE = "application/x-mumcord-channel";

/**
 * The channel being dragged, while a drag is on. Rows need it during `dragover`, where the
 * browser hides the dragged data, and to dim the dragged branch.
 */
export const draggedChannel = valueStore<number | null>(null);

export type DropZone = "above" | "into" | "below";

/** Which part of a row the pointer is over: the top and bottom quarters place, the rest moves. */
export function dropZone(offsetY: number, height: number): DropZone {
  if (offsetY < height / 4) return "above";
  if (offsetY > (height * 3) / 4) return "below";
  return "into";
}

/** One `ChannelState` edit; `null` leaves that value as it is. */
export interface ChannelEditStep {
  channelId: number;
  parentId: number | null;
  position: number | null;
}

export type DropPlan =
  /** Dropping there would put the channel into itself or one of its own sub-channels. */
  | { kind: "refused"; reason: "self" | "subChannel" }
  /** Nothing would change, or the target cannot take channels. */
  | { kind: "none" }
  | {
      kind: "move";
      channelId: number;
      fromParentId: number;
      toParentId: number;
      /** Into the target row's channel, or above or below it among its siblings. */
      zone: DropZone;
      /** The dragged channel's edit first, then any siblings that need a new position. */
      steps: ChannelEditStep[];
    };

export type ChannelMovePlan = Extract<DropPlan, { kind: "move" }>;

/** True when `channelId` is `ancestorId` or lies somewhere below it. */
export function isWithin(
  channels: ReadonlyMap<number, ChannelInfo>,
  channelId: number,
  ancestorId: number,
): boolean {
  const seen = new Set<number>();
  let current: number | null = channelId;
  // `seen` guards against a broken tree from the server looping forever.
  while (current !== null && !seen.has(current)) {
    if (current === ancestorId) return true;
    seen.add(current);
    current = channels.get(current)?.parentId ?? null;
  }
  return false;
}

/** What dropping `draggedId` on `zone` of the row of `targetId` does. */
export function planChannelDrop(
  channels: ReadonlyMap<number, ChannelInfo>,
  draggedId: number,
  targetId: number,
  zone: DropZone,
): DropPlan {
  const dragged = channels.get(draggedId);
  const target = channels.get(targetId);
  if (dragged?.parentId == null || target === undefined) return { kind: "none" };
  // The root has no siblings: anything dropped on its row goes into it.
  const placing = zone !== "into" && target.parentId !== null;
  if (placing && targetId === draggedId) return { kind: "none" };
  if (isWithin(channels, targetId, draggedId)) {
    return { kind: "refused", reason: targetId === draggedId ? "self" : "subChannel" };
  }
  const fromParentId = dragged.parentId;
  if (!placing) {
    // A temporary channel cannot hold sub-channels.
    if (targetId === fromParentId || target.temporary) return { kind: "none" };
    return {
      kind: "move",
      channelId: draggedId,
      fromParentId,
      toParentId: targetId,
      zone: "into",
      steps: [{ channelId: draggedId, parentId: targetId, position: null }],
    };
  }
  const toParentId = target.parentId ?? fromParentId;
  const siblings = [...channels.values()]
    .filter((c) => c.parentId === toParentId && c.id !== draggedId)
    .sort(compareChannels);
  const index = siblings.findIndex((c) => c.id === targetId) + (zone === "below" ? 1 : 0);
  const positions = positionsFor(siblings, index, dragged);
  const parentId = toParentId === fromParentId ? null : toParentId;
  const position = positions.get(draggedId) ?? null;
  if (parentId === null && position === null && positions.size === 0) return { kind: "none" };
  const steps = [
    { channelId: draggedId, parentId, position },
    ...[...positions]
      .filter(([id]) => id !== draggedId)
      .map(([channelId, value]) => ({ channelId, parentId: null, position: value })),
  ];
  return { kind: "move", channelId: draggedId, fromParentId, toParentId, zone, steps };
}

/**
 * New positions that sort `channel` at `index` among `siblings` (sorted, without it), as few as
 * possible: preferably only the channel's own, else the whole row renumbered in its new order.
 * Returns only the positions that change.
 */
function positionsFor(
  siblings: readonly ChannelInfo[],
  index: number,
  channel: ChannelInfo,
): Map<number, number> {
  const before = siblings[index - 1];
  const after = siblings[index];
  const candidates = [
    channel.position,
    ...(before ? [before.position, before.position + 1] : []),
    ...(after ? [after.position - 1, after.position] : []),
  ];
  const fits = (position: number) => {
    const placed = { name: channel.name, position };
    return (
      (before === undefined || compareChannels(before, placed) < 0) &&
      (after === undefined || compareChannels(placed, after) < 0)
    );
  };
  const own = candidates.find(fits);
  if (own !== undefined) {
    return new Map(own === channel.position ? [] : [[channel.id, own]]);
  }
  const order = [...siblings.slice(0, index), channel, ...siblings.slice(index)];
  return new Map(
    order.flatMap((c, position) => (c.position === position ? [] : [[c.id, position] as const])),
  );
}

/** The names from the top channel below the root down to `channelId`, e.g. Gaming › Raid. */
export function channelNamePath(
  channels: ReadonlyMap<number, ChannelInfo>,
  channelId: number,
): string[] {
  const names: string[] = [];
  const seen = new Set<number>();
  let current = channels.get(channelId);
  while (current?.parentId != null && !seen.has(current.id)) {
    seen.add(current.id);
    names.unshift(current.name);
    current = channels.get(current.parentId);
  }
  return names;
}
