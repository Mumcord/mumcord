/**
 * The tray's unread badge and "Recent chats" menu (FR-3.132). Unread counts and mentions are
 * only known here, so the UI computes the summary and hands it to the tray.
 */
import type { Friend, TrayAttention } from "../../api";
import { collectEntries, type Entry } from "../follow/directEntries";
import type { FollowState } from "../follow/followStore";
import { mentionCount } from "../servers/mentions";

/** Recent chats in the tray menu. */
export const MAX_RECENT_CHATS = 5;

export interface AttentionInput {
  states: ReadonlyMap<number, FollowState>;
  mentions: ReadonlyMap<number, ReadonlyMap<string, number>>;
  /** Saved servers in server-bar order. */
  servers: readonly { id: number; notificationsMuted: boolean }[];
  friends: readonly Friend[];
  nameOf: (entry: Entry) => string;
}

/**
 * Unread messages and mentions of every server whose notifications are not muted (FR-2.10),
 * and the most recent private chats, those with unread messages first.
 */
export function trayAttention({
  states,
  mentions,
  servers,
  friends,
  nameOf,
}: AttentionInput): TrayAttention {
  const muted = new Set(servers.filter((s) => s.notificationsMuted).map((s) => s.id));
  let unread = 0;
  let mentioned = 0;
  for (const [serverId, state] of states) {
    if (muted.has(serverId)) continue;
    for (const count of state.unread.values()) unread += count;
    mentioned += mentionCount(mentions.get(serverId), state);
  }
  // Chats are kept in the order they were opened, so the newest is last.
  const entries = collectEntries(
    states,
    servers.map((s) => s.id),
    friends,
  ).reverse();
  const recent = [...entries.filter((e) => e.unread > 0), ...entries.filter((e) => e.unread === 0)];
  return {
    unread,
    mentions: mentioned,
    chats: recent.slice(0, MAX_RECENT_CHATS).map((entry) => ({
      serverId: entry.serverId,
      conversation: entry.chat.conversation,
      name: nameOf(entry),
      unread: entry.unread,
    })),
  };
}
