/**
 * Whether a message mentions the user: by the current name, or by a name they had earlier in this
 * session (renaming must not hide mentions that were meant for them), by replying to one of
 * their messages (FR-2.67), or by `@all` / `@channel` unless turned off for the server (#70).
 */
import { useMemo } from "react";
import type { ChatMessage } from "../../api";
import {
  getServerState,
  getServerStateOf,
  selfUser,
  shownServerId,
  subscribeServerChanges,
  useServerSelector,
  type ServerState,
} from "../channels";
import { groupMentionsMuted, useServers } from "../servers/store";
import { conversationMessages, useConversationMessages } from "./chatStore";
import { bodyMentions, groupMentioned } from "./mentions";
import { repliesToOwn } from "./reply";

let previousNames = new Set<string>();
let tracking = false;

function track(): void {
  if (tracking) return;
  tracking = true;
  subscribeServerChanges((change, state) => {
    if (change.kind === "synced") {
      previousNames = new Set();
    } else if (
      change.kind === "userUpserted" &&
      change.user.session === state.selfSession &&
      change.previous !== undefined &&
      change.previous.name !== change.user.name
    ) {
      previousNames.add(change.previous.name);
    }
  });
}

/** Names that count as "me" in a mention, current name first. */
export function ownMentionNames(): string[] {
  return mentionNames(selfUser(getServerState())?.name);
}

function mentionNames(name: string | undefined): string[] {
  track();
  return [...(name === undefined ? [] : [name]), ...previousNames];
}

/** True when `message` on `serverId` replies to one of the user's own messages there. */
export function isReplyMention(message: ChatMessage, serverId: number): boolean {
  const name = selfUser(getServerStateOf(serverId))?.name;
  return (
    name !== undefined &&
    repliesToOwn(message, name, conversationMessages(message.conversation, serverId))
  );
}

/** True when `message` on `serverId` reaches the user through `@all` or `@channel`. */
export function isGroupMention(message: ChatMessage, serverId: number): boolean {
  return groupMentioned(message.body, message.conversation, {
    ownChannel: selfUser(getServerStateOf(serverId))?.channelId ?? null,
    muted: groupMentionsMuted(serverId),
  });
}

/**
 * True when `message` is from someone else and mentions the user. Replies and group mentions
 * are checked on `serverId` (default: the shown server).
 */
export function isMention(message: ChatMessage, serverId = shownServerId()): boolean {
  if (message.own) return false;
  return (
    bodyMentions(message.body, ownMentionNames()) ||
    isReplyMention(message, serverId) ||
    isGroupMention(message, serverId)
  );
}

/**
 * Re-evaluates when the user's name or the loaded history changes. Reads only what the hooks
 * render with: `isMention` applies queued server events, which would update other components
 * in the middle of this render.
 */
export function useIsMention(message: ChatMessage): boolean {
  const name = useServerSelector(selfName);
  const ownChannel = useServerSelector(selfChannel);
  const history = useConversationMessages(message.conversation);
  const shown = shownServerId();
  const muted = useServers().servers.find((s) => s.id === shown)?.groupMentionsMuted ?? false;
  return useMemo(() => {
    if (message.own) return false;
    return (
      bodyMentions(message.body, mentionNames(name)) ||
      (name !== undefined && repliesToOwn(message, name, history)) ||
      groupMentioned(message.body, message.conversation, { ownChannel, muted })
    );
  }, [message, name, history, ownChannel, muted]);
}

function selfChannel(state: ServerState): number | null {
  return selfUser(state)?.channelId ?? null;
}

function selfName(state: ServerState): string | undefined {
  return selfUser(state)?.name;
}
