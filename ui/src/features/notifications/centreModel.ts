/**
 * Data model of the notification centre (#143, FR-3.105–3.107): what an entry is, which filter it
 * belongs to, the counts and the unread badge. Pure functions only; `centreStore` keeps the state.
 *
 * Mentions, replies, invites and connection events are collected as they happen and live in
 * memory for the app run. Chat and message requests are not copied: they come from the requests
 * store (#141), which already drops them on disconnect and quit; the centre only remembers which
 * of them were read or dismissed.
 */
import type { Conversation, MessageRequest, PmRequests } from "../../api";

/** Mentions and replies point at stored messages and are kept this long (FR-3.107). */
export const MESSAGE_ITEM_TTL_MS = 7 * 24 * 60 * 60 * 1000;

export type CentreFilter = "all" | "mentions" | "requests" | "invites";

export const FILTERS: readonly CentreFilter[] = ["all", "mentions", "requests", "invites"];

interface Base {
  /** Unique across the centre; see the `*Id` helpers. */
  id: string;
  serverId: number;
  /** Unix milliseconds; the list is newest first. */
  at: number;
  read: boolean;
}

/** An entry that points at one chat message. */
interface MessageRef {
  conversation: Conversation;
  messageId: string;
  /** Name the sender wrote under; untrusted plain text. */
  sender: string;
  /** Plain-text excerpt; untrusted. */
  excerpt: string;
}

export type MentionItem = Base &
  MessageRef & {
    kind: "mention";
    /** `@all` / `@channel` (#70) rather than the user's name. */
    group: boolean;
  };

export type ReplyItem = Base & MessageRef & { kind: "reply" };

/** A message card or a shared theme sent in a private chat. */
export type CardInviteItem = Base &
  MessageRef & {
    kind: "invite";
    invite: "card" | "theme";
  };

/** A watch party started in the user's channel that they have not joined (FR-2.39). */
export type WatchPartyInviteItem = Base & {
  kind: "invite";
  invite: "watchParty";
  sender: string;
  channelId: number;
};

/** Someone in the user's voice channel shares their screen (FR-2.42). */
export type ScreenShareInviteItem = Base & {
  kind: "invite";
  invite: "screenShare";
  sender: string;
  session: number;
};

export type InviteItem = CardInviteItem | WatchPartyInviteItem | ScreenShareInviteItem;

/** The banner of an ended connection hid itself unseen (#88, Q1.18). */
export type ConnectionItem = Base & {
  kind: "connection";
  /** The banner's text, already translated. */
  text: string;
};

/** Everything the centre stores itself. */
export type StoredItem = MentionItem | ReplyItem | InviteItem | ConnectionItem;

/** A held chat or message request (#141), built from the requests store on every render. */
export type RequestItem = Base & { kind: "request"; request: MessageRequest };

export type CentreItem = StoredItem | RequestItem;

export interface CentreState {
  /** Stored entries in arrival order. */
  items: readonly StoredItem[];
  /** Ids (`requestItemId`) of requests marked read. */
  readRequests: ReadonlySet<string>;
  /** Ids of requests removed from the centre; they stay in the requests view. */
  dismissedRequests: ReadonlySet<string>;
}

export function messageItemId(messageId: string): string {
  return `message/${messageId}`;
}

export function requestItemId(serverId: number, requestId: number): string {
  return `request/${String(serverId)}/${String(requestId)}`;
}

export function watchPartyItemId(serverId: number): string {
  return `watchParty/${String(serverId)}`;
}

export function screenShareItemId(serverId: number, session: number): string {
  return `screenShare/${String(serverId)}/${String(session)}`;
}

/** The filter chip an entry counts under; connection events only show under "All". */
export function filterOf(item: CentreItem): Exclude<CentreFilter, "all"> | null {
  switch (item.kind) {
    case "mention":
    case "reply":
      return "mentions";
    case "request":
      return "requests";
    case "invite":
      return "invites";
    case "connection":
      return null;
  }
}

/** True when the entry links to a chat message. */
export function hasMessage(item: CentreItem): item is MentionItem | ReplyItem | CardInviteItem {
  return "messageId" in item;
}

/** Stored entries and the open requests of every server, newest first. */
export function buildItems(
  state: CentreState,
  requests: ReadonlyMap<number, PmRequests>,
): CentreItem[] {
  const all: CentreItem[] = [...state.items];
  for (const [serverId, { incoming }] of requests) {
    for (const request of incoming) {
      const id = requestItemId(serverId, request.id);
      if (state.dismissedRequests.has(id)) continue;
      const at = Date.parse(request.receivedAt);
      all.push({
        kind: "request",
        id,
        serverId,
        at: Number.isNaN(at) ? 0 : at,
        read: state.readRequests.has(id),
        request,
      });
    }
  }
  // Stable for equal times: the later arrival comes first.
  return all
    .map((item, index) => ({ item, index }))
    .sort((a, b) => b.item.at - a.item.at || b.index - a.index)
    .map(({ item }) => item);
}

export function visibleItems(items: readonly CentreItem[], filter: CentreFilter): CentreItem[] {
  return filter === "all" ? [...items] : items.filter((item) => filterOf(item) === filter);
}

/** Number of entries per filter chip ("Mentions 2"). */
export function filterCounts(items: readonly CentreItem[]): Record<CentreFilter, number> {
  const counts: Record<CentreFilter, number> = {
    all: items.length,
    mentions: 0,
    requests: 0,
    invites: 0,
  };
  for (const item of items) {
    const filter = filterOf(item);
    if (filter !== null) counts[filter] += 1;
  }
  return counts;
}

/** The bell's badge. Do Not Disturb does not change it: everything is still collected (Q1.5). */
export function unreadCount(items: readonly CentreItem[]): number {
  return items.reduce((sum, item) => sum + (item.read ? 0 : 1), 0);
}

/** Drops mentions and replies older than `MESSAGE_ITEM_TTL_MS`; the rest stays for the app run. */
export function prune(items: readonly StoredItem[], now: number): readonly StoredItem[] {
  const oldest = now - MESSAGE_ITEM_TTL_MS;
  const kept = items.filter(
    (item) => !((item.kind === "mention" || item.kind === "reply") && item.at < oldest),
  );
  return kept.length === items.length ? items : kept;
}
