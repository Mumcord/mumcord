import type { ChatMessage } from "../../api";

/** Consecutive messages of one sender within this time share one name/time header. */
export const GROUP_GAP_MS = 5 * 60 * 1000;

function sameSender(a: ChatMessage, b: ChatMessage): boolean {
  if (a.own || b.own) return a.own && b.own;
  if (a.senderCertHash !== null || b.senderCertHash !== null) {
    return a.senderCertHash === b.senderCertHash;
  }
  return a.senderSession === b.senderSession && a.senderName === b.senderName;
}

/** For each message: does it start a new group (shows sender and time)? */
export function groupStarts(messages: readonly ChatMessage[]): boolean[] {
  return messages.map((message, index) => {
    const previous = index > 0 ? messages[index - 1] : undefined;
    if (!previous || !sameSender(previous, message)) return true;
    return Date.parse(message.sentAt) - Date.parse(previous.sentAt) > GROUP_GAP_MS;
  });
}
