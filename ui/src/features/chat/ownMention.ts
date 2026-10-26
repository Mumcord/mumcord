/**
 * Whether a message mentions the user: by the current name, or by a name they had earlier in this
 * session (renaming must not hide mentions that were meant for them), or by replying to one of
 * their messages (FR-2.67).
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
import { conversationMessages, useConversationMessages } from "./chatStore";
import { bodyMentions } from "./mentions";
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

/**
 * True when `message` is from someone else and mentions the user. Replies are checked on
 * `serverId` (default: the shown server).
 */
export function isMention(message: ChatMessage, serverId = shownServerId()): boolean {
  if (message.own) return false;
  return bodyMentions(message.body, ownMentionNames()) || isReplyMention(message, serverId);
}

/**
 * Re-evaluates when the user's name or the loaded history changes. Reads only what the hooks
 * render with: `isMention` applies queued server events, which would update other components
 * in the middle of this render.
 */
export function useIsMention(message: ChatMessage): boolean {
  const name = useServerSelector(selfName);
  const history = useConversationMessages(message.conversation);
  return useMemo(() => {
    if (message.own) return false;
    return (
      bodyMentions(message.body, mentionNames(name)) ||
      (name !== undefined && repliesToOwn(message, name, history))
    );
  }, [message, name, history]);
}

function selfName(state: ServerState): string | undefined {
  return selfUser(state)?.name;
}
