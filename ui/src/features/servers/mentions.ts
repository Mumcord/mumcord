/**
 * Mentions per connected server for the server bar's mention count (spec §5.2). The engine only
 * reports unread counts, so the mentions among them are counted here from incoming messages. A
 * conversation's count resets when the engine reports it read (unread count 0); a mention in a
 * chat that is on screen is marked read right away and so never shows.
 */
import { useSyncExternalStore } from "react";
import { getApi, type MumcordApi, type Unsubscribe } from "../../api";
import { getServerStateOf, selfUser } from "../channels/serverStore";
import { bodyMentions, groupMentioned } from "../chat/mentions";
import { conversationKey, conversationMessages } from "../chat/chatStore";
import { repliesToOwn } from "../chat/reply";
import type { FollowState } from "../follow/followStore";
import { groupMentionsMuted } from "./store";

type Mentions = ReadonlyMap<number, ReadonlyMap<string, number>>;

let mentions: Mentions = new Map();
let boundApi: MumcordApi | undefined;
let unsubscribers: Unsubscribe[] = [];
const listeners = new Set<() => void>();

function update(serverId: number, change: (current: Map<string, number>) => void): void {
  const next = new Map(mentions.get(serverId));
  change(next);
  const all = new Map(mentions);
  if (next.size === 0) all.delete(serverId);
  else all.set(serverId, next);
  mentions = all;
  for (const listener of listeners) listener();
}

function bind(api: MumcordApi): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  boundApi = api;
  mentions = new Map();
  unsubscribers = [
    api.on("messageReceived", ({ serverId, message }) => {
      const self = selfUser(getServerStateOf(serverId));
      if (message.own || self === undefined) return;
      const { name, channelId } = self;
      const history = conversationMessages(message.conversation, serverId);
      const group = { ownChannel: channelId, muted: groupMentionsMuted(serverId) };
      if (
        !bodyMentions(message.body, [name]) &&
        !repliesToOwn(message, name, history) &&
        !groupMentioned(message.body, message.conversation, group)
      ) {
        return;
      }
      const key = conversationKey(message.conversation);
      update(serverId, (current) => current.set(key, (current.get(key) ?? 0) + 1));
    }),
    api.on("unreadChanged", ({ serverId, conversation, count }) => {
      const key = conversationKey(conversation);
      if (count === 0 && mentions.get(serverId)?.has(key)) {
        update(serverId, (current) => current.delete(key));
      }
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state === "disconnected" && mentions.has(serverId)) {
        update(serverId, (current) => {
          current.clear();
        });
      }
    }),
  ];
}

function subscribe(listener: () => void): () => void {
  const api = getApi();
  if (api !== boundApi) bind(api);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getMentions(): Mentions {
  return mentions;
}

export function useMentions(): Mentions {
  return useSyncExternalStore(subscribe, getMentions);
}

/** Unread mentions of one server: at most the unread count of each conversation. */
export function mentionCount(
  serverMentions: ReadonlyMap<string, number> | undefined,
  follow: FollowState | undefined,
): number {
  let total = 0;
  for (const [key, count] of serverMentions ?? []) {
    total += Math.min(count, follow?.unread.get(key) ?? 0);
  }
  return total;
}

/** Test helper. */
export function resetMentions(): void {
  for (const unsubscribe of unsubscribers) unsubscribe();
  unsubscribers = [];
  boundApi = undefined;
  mentions = new Map();
}
