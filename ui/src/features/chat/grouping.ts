import type { ChatMessage } from "../../api";

/** Consecutive messages of one sender within this time form one group (one avatar and name). */
export const GROUP_GAP_MS = 5 * 60 * 1000;

function sameSender(a: ChatMessage, b: ChatMessage): boolean {
  if (a.own || b.own) return a.own && b.own;
  if (a.senderCertHash !== null || b.senderCertHash !== null) {
    return a.senderCertHash === b.senderCertHash;
  }
  return a.senderSession === b.senderSession && a.senderName === b.senderName;
}

/** Local calendar day as a whole number, so days can be compared and subtracted. */
function localDay(date: Date): number {
  return Math.round(
    new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime() / 86_400_000,
  );
}

/** How many local calendar days `date` lies before `now` (0 = today, 1 = yesterday). */
export function daysAgo(date: Date, now: Date): number {
  return localDay(now) - localDay(date);
}

export interface MessageLayout {
  /** Starts a new group: shows avatar, name and time. */
  start: boolean;
  /** First message of a local calendar day: a day separator goes above it. */
  newDay: boolean;
}

/**
 * Layout of each message: a group ends when the sender changes, after a gap of more than
 * `GROUP_GAP_MS`, or at a day boundary (the separator sits between them).
 */
export function messageLayout(messages: readonly ChatMessage[]): MessageLayout[] {
  return messages.map((message, index) => {
    const previous = index > 0 ? messages[index - 1] : undefined;
    const sentAt = new Date(message.sentAt);
    if (!previous) return { start: true, newDay: true };
    const before = new Date(previous.sentAt);
    const newDay = localDay(sentAt) !== localDay(before);
    const start =
      newDay ||
      !sameSender(previous, message) ||
      sentAt.getTime() - before.getTime() > GROUP_GAP_MS;
    return { start, newDay };
  });
}
