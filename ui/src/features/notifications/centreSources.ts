/**
 * Where the notification centre's entries come from (#143): incoming messages (mentions, replies,
 * cards and themes sent privately), watch parties and screen shares starting in the user's
 * channel, and connection banners that hid themselves (Q1.18). Requests come from the requests
 * store directly (see `centreModel`).
 */
import type { ChatMessage, MumcordApi, Unsubscribe, WatchParty } from "../../api";
import { isBlocked } from "../blocking/blockedStore";
import { getServerStateOf, selfUser } from "../channels";
import { extractCards } from "../chat/cards";
import { isGroupMention, isMention, isReplyMention } from "../chat/ownMention";
import { messageText } from "../chat/reply";
import { subscribeEndedHidden } from "../servers/endedEvents";
import { findThemeCode } from "../../theme/shareCodec";
import {
  messageItemId,
  screenShareItemId,
  UPDATE_ITEM_ID,
  watchPartyItemId,
  type StoredItem,
} from "./centreModel";
import { addItem, getCentreState, hasItem, removeItems } from "./centreStore";
import { MAX_PREVIEW, plainPreview } from "./preview";

function capped(text: string): string {
  const chars = Array.from(text.replace(/\s+/g, " ").trim());
  return chars.length <= MAX_PREVIEW
    ? chars.join("")
    : `${chars
        .slice(0, MAX_PREVIEW - 1)
        .join("")
        .trimEnd()}…`;
}

function timeOf(message: ChatMessage): number {
  const at = Date.parse(message.sentAt);
  return Number.isNaN(at) ? Date.now() : at;
}

/**
 * Collects an incoming message that wants the user's attention. Called from the same path as
 * toasts, before Do Not Disturb and mute decide about sound and toast: the centre always
 * collects (Q1.5). A message in the chat on screen is collected as read.
 */
export function recordIncoming(message: ChatMessage, serverId: number, onScreen: boolean): void {
  if (message.own) return;
  // Nothing from people the user blocked (issue #140), not even mentions.
  if (isBlocked(serverId, message.senderCertHash, message.senderName)) return;
  const base = {
    id: messageItemId(message.id),
    serverId,
    at: timeOf(message),
    read: onScreen,
    conversation: message.conversation,
    messageId: message.id,
    sender: message.senderName,
    senderCertHash: message.senderCertHash,
  };
  if (isReplyMention(message, serverId)) {
    addItem({ ...base, kind: "reply", excerpt: capped(messageText(message)) });
    return;
  }
  if (isMention(message, serverId)) {
    addItem({
      ...base,
      kind: "mention",
      group: isGroupMention(message, serverId),
      excerpt: plainPreview(message.body),
    });
    return;
  }
  // Cards and themes in a channel are for everyone; only a private one is an invite.
  if (message.conversation.kind === "channel") return;
  const [card] = extractCards(message.body).cards;
  if (card) {
    addItem({ ...base, kind: "invite", invite: "card", excerpt: capped(card.title ?? "") });
  } else if (findThemeCode(message.body) !== null) {
    addItem({ ...base, kind: "invite", invite: "theme", excerpt: "" });
  }
}

/**
 * An installed update waits for a restart (#168). Replaces the entry of an older version; the
 * same version again keeps the entry as it is, read or unread.
 */
export function recordUpdateReady(version: string, url: string): void {
  const current = getCentreState().items.find((item) => item.id === UPDATE_ITEM_ID);
  if (current?.kind === "update" && current.version === version) return;
  addItem({ kind: "update", id: UPDATE_ITEM_ID, at: Date.now(), read: false, version, url });
}

/** The updater left the "ready" phase, e.g. a newer download failed: the entry no longer holds. */
export function clearUpdateReady(): void {
  removeItems((item) => item.kind === "update");
}

/** Name and certificate of a person in the server, for invites they started. */
function senderOf(
  serverId: number,
  session: number | undefined,
): { sender: string; senderCertHash: string | null } {
  const user = session === undefined ? undefined : getServerStateOf(serverId).users.get(session);
  return { sender: user?.name ?? "", senderCertHash: user?.certHash ?? null };
}

/**
 * Follows watch parties, screen shares and hidden connection banners while the app runs.
 * Returns the function that stops it.
 */
export function bindCentreSources(api: MumcordApi): Unsubscribe {
  /** Servers with a running party, so later changes of the same party add nothing. */
  const parties = new Set<number>();
  /** Sharing sessions per server, so only a newly started share is an invite. */
  const shares = new Map<number, ReadonlySet<number>>();

  function partyChanged(serverId: number, party: WatchParty | null): void {
    const id = watchPartyItemId(serverId);
    if (party === null || party.joined) {
      // Ended or joined: the invitation is answered.
      if (party === null) parties.delete(serverId);
      else parties.add(serverId);
      removeItems((item) => item.id === id);
      return;
    }
    if (parties.has(serverId)) return;
    parties.add(serverId);
    const state = getServerStateOf(serverId);
    if (selfUser(state)?.channelId !== party.channelId || hasItem(id)) return;
    const from = senderOf(serverId, party.participants[0]);
    if (isBlocked(serverId, from.senderCertHash, from.sender)) return;
    addItem({
      kind: "invite",
      invite: "watchParty",
      id,
      serverId,
      at: Date.now(),
      read: false,
      ...from,
      channelId: party.channelId,
    });
  }

  function sharesChanged(serverId: number, sessions: readonly number[]): void {
    const before = shares.get(serverId) ?? new Set<number>();
    const now = new Set(sessions);
    shares.set(serverId, now);
    const self = getServerStateOf(serverId).selfSession;
    for (const session of now) {
      if (before.has(session) || session === self) continue;
      const from = senderOf(serverId, session);
      if (isBlocked(serverId, from.senderCertHash, from.sender)) continue;
      addItem({
        kind: "invite",
        invite: "screenShare",
        id: screenShareItemId(serverId, session),
        serverId,
        at: Date.now(),
        read: false,
        ...from,
        session,
      });
    }
    const ended = new Set([...before].filter((session) => !now.has(session)));
    if (ended.size > 0) {
      removeItems(
        (item) =>
          item.kind === "invite" &&
          item.invite === "screenShare" &&
          item.serverId === serverId &&
          ended.has(item.session),
      );
    }
  }

  // Live invites belong to one connection; messages and connection events stay.
  function isLive(item: StoredItem, serverId: number): boolean {
    return (
      item.kind === "invite" &&
      item.serverId === serverId &&
      (item.invite === "watchParty" || item.invite === "screenShare")
    );
  }

  const unsubscribers = [
    api.on("watchPartyChanged", ({ serverId, party }) => {
      partyChanged(serverId, party);
    }),
    api.on("screenSharesChanged", ({ serverId, shares: list }) => {
      sharesChanged(
        serverId,
        list.map((share) => share.session),
      );
    }),
    api.on("connectionStateChanged", ({ serverId, state }) => {
      if (state.state === "connected") return;
      parties.delete(serverId);
      shares.delete(serverId);
      removeItems((item) => isLive(item, serverId));
    }),
    subscribeEndedHidden(({ end, text }) => {
      addItem({
        kind: "connection",
        id: `connection/${String(end.serverId)}/${String(Date.now())}`,
        serverId: end.serverId,
        at: Date.now(),
        read: false,
        text,
      });
    }),
  ];
  return () => {
    for (const unsubscribe of unsubscribers) unsubscribe();
  };
}
