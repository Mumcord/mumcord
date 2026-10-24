/**
 * Whether a message mentions the user: by the current name, or by a name they had earlier in this
 * session (renaming must not hide mentions that were meant for them).
 */
import { useMemo } from "react";
import type { ChatMessage } from "../../api";
import { getServerState, selfUser, subscribeServerChanges, useServerSelector } from "../channels";
import { bodyMentions } from "./mentions";

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
  track();
  const name = selfUser(getServerState())?.name;
  return [...(name === undefined ? [] : [name]), ...previousNames];
}

/** True when `message` is from someone else and mentions the user. */
export function isMention(message: ChatMessage): boolean {
  return !message.own && bodyMentions(message.body, ownMentionNames());
}

/** Re-evaluates when the user's name changes. */
export function useIsMention(message: ChatMessage): boolean {
  const name = useServerSelector((state) => selfUser(state)?.name ?? "");
  // `name` is no input of the computation but changes what `isMention` reads from the store.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => isMention(message), [message, name]);
}
