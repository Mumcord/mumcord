/**
 * What the Channel window and the delete confirmation work out before anything is sent (Epic 4,
 * issue #250): which parents a channel may move under, what a delete affects, and what an edit
 * changed. The server has the final say on every change.
 */
import type { TFunction } from "i18next";
import type { ChannelChangeInput, ChannelInfo, ChannelResult, User } from "../../api";

/** A channel's sub-channels at every depth, without the channel itself. */
export function descendants(
  channels: ReadonlyMap<number, ChannelInfo>,
  channelId: number,
): Set<number> {
  const found = new Set<number>();
  let frontier = [channelId];
  while (frontier.length > 0) {
    const next: number[] = [];
    for (const channel of channels.values()) {
      if (
        channel.parentId !== null &&
        frontier.includes(channel.parentId) &&
        !found.has(channel.id)
      ) {
        found.add(channel.id);
        next.push(channel.id);
      }
    }
    frontier = next;
  }
  return found;
}

/** What deleting a channel takes with it: its sub-channels and the people moved to the parent. */
export function deleteImpact(
  channels: ReadonlyMap<number, ChannelInfo>,
  users: ReadonlyMap<number, User>,
  channelId: number,
): { subChannels: number; people: number } {
  const below = descendants(channels, channelId);
  const affected = new Set([channelId, ...below]);
  let people = 0;
  for (const user of users.values()) if (affected.has(user.channelId)) people += 1;
  return { subChannels: below.size, people };
}

/**
 * Parents a channel may move under (FR-10.24): never itself or one of its sub-channels, which
 * would make a loop. Temporary channels cannot hold sub-channels.
 */
export function parentOptions(
  channels: ReadonlyMap<number, ChannelInfo>,
  channelId: number | null,
): ChannelInfo[] {
  const excluded =
    channelId === null
      ? new Set<number>()
      : new Set([channelId, ...descendants(channels, channelId)]);
  return [...channels.values()].filter((c) => !excluded.has(c.id) && !c.temporary);
}

/** The General tab's fields, as the user edits them. */
export interface ChannelDraft {
  name: string;
  /** Clean description HTML; empty for none. */
  description: string;
  parentId: number;
  position: number;
  /** 0 is the server's own limit. */
  maxUsers: number;
  temporary: boolean;
  links: number[];
}

/** The draft for an existing channel, or a new one under `parentId`. */
export function draftOf(
  channel: ChannelInfo | null,
  parentId: number,
  temporary = false,
): ChannelDraft {
  return {
    name: channel?.name ?? "",
    description: channel?.description ?? "",
    parentId: channel?.parentId ?? parentId,
    position: channel?.position ?? 0,
    maxUsers: channel?.maxUsers ?? 0,
    temporary: channel?.temporary ?? temporary,
    links: channel?.links ?? [],
  };
}

/** Only what the user changed is sent, so an edit never overwrites someone else's change. */
export function changeOf(before: ChannelDraft, after: ChannelDraft): ChannelChangeInput {
  const linksAdd = after.links.filter((id) => !before.links.includes(id));
  const linksRemove = before.links.filter((id) => !after.links.includes(id));
  return {
    name: after.name.trim() === before.name ? null : after.name.trim(),
    description: after.description === before.description ? null : after.description,
    parentId: after.parentId === before.parentId ? null : after.parentId,
    position: after.position === before.position ? null : after.position,
    maxUsers: after.maxUsers === before.maxUsers ? null : after.maxUsers,
    linksAdd,
    linksRemove,
  };
}

export function hasChanges(change: ChannelChangeInput): boolean {
  return (
    change.name !== null ||
    change.description !== null ||
    change.parentId !== null ||
    change.position !== null ||
    change.maxUsers !== null ||
    change.linksAdd.length > 0 ||
    change.linksRemove.length > 0
  );
}

/** The plain-language text of a refused change; `null` when the server applied or took it. */
export function refusalText(t: TFunction<"admin">, result: ChannelResult): string | null {
  if (result.kind !== "refused") return null;
  switch (result.denial) {
    case "permission":
      return result.permission === null
        ? t("denied.permission")
        : t("denied.sentence", { action: t(`denied.right.${result.permission}`) });
    case "text":
    case "unknown":
      return result.reason ?? t("denied.unknown");
    default:
      return t(`denied.kind.${result.denial}`);
  }
}
