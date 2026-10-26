/**
 * Turns one incoming message into a toast or system notification (FR-1.48c). The engine decides
 * by window focus and setting (`notifyMessage`); this side knows which chat is on screen and
 * builds the translated, plain-text texts.
 */
import type { TFunction } from "i18next";
import { getApi, type ChatMessage, type Conversation } from "../../api";
import {
  getSelectedConversation,
  getServerStateOf,
  getShownServerId,
  sameConversation,
} from "../channels";
import { conversationKey, isMention } from "../chat";
import { bumpCount, markMention, markSeen, noticeKey, showToast } from "./notificationStore";
import { hasImage, plainPreview } from "./preview";

export interface IncomingContext {
  t: TFunction<"notifications">;
  /** A full-window overlay (settings) covers the chat. */
  overlayOpen: boolean;
  /** Saved name of a server (FR-2.10), `undefined` if unknown. */
  serverName: (serverId: number) => string | undefined;
  /** Notifications of this server are muted (FR-2.10). */
  muted: (serverId: number) => boolean;
}

export interface NoticeTexts {
  summary: string;
  body: string;
}

/** Title and line for a chat with `count` unseen messages, the newest being `message`. */
export function noticeTexts(
  message: ChatMessage,
  count: number,
  t: TFunction<"notifications">,
  serverId: number,
  server: string | undefined,
  mentioned = false,
): NoticeTexts {
  const { conversation } = message;
  const isChannel = conversation.kind === "channel";
  const name = isChannel ? channelLabel(serverId, conversation.channelId, t) : message.senderName;
  const what =
    count === 1 ? name : t(isChannel ? "groupedChannel" : "groupedPrivate", { count, name });
  const summary = server === undefined ? what : t("withServer", { server, what });
  const preview = plainPreview(message.body) || (hasImage(message.body) ? t("image") : "");
  let body = preview;
  if (mentioned) body = t("mentionLine", { sender: message.senderName, preview });
  else if (isChannel) body = t("line", { sender: message.senderName, preview });
  return { summary, body };
}

function channelLabel(serverId: number, channelId: number, t: TFunction<"notifications">): string {
  const channel = getServerStateOf(serverId).channels.get(channelId);
  return channel ? t("channel", { name: channel.name }) : t("unknownChannel");
}

/** True when the UI shows this chat right now (window focus is checked by the engine). */
function isOnScreen(serverId: number, conversation: Conversation, overlayOpen: boolean): boolean {
  return (
    !overlayOpen &&
    serverId === getShownServerId() &&
    sameConversation(getSelectedConversation(), conversation)
  );
}

async function mentionNotifications(): Promise<boolean> {
  try {
    return (await getApi().settingsGet()).sounds.mentionNotify;
  } catch (error) {
    console.error("Reading the mention notification setting failed", error);
    return true;
  }
}

export async function handleIncoming(
  message: ChatMessage,
  serverId: number,
  context: IncomingContext,
): Promise<void> {
  if (message.own || context.muted(serverId)) return;
  const { conversation } = message;
  const key = noticeKey(serverId, conversationKey(conversation));
  const onScreen = isOnScreen(serverId, conversation, context.overlayOpen);
  const count = bumpCount(key);
  // A mention notifies like a private message: the setting decides how, the chat does not matter.
  // The ping (engine, in `notifyMessage`) follows the mention itself; only the highlighting of
  // the notification follows the "Notify when mentioned" switch.
  const mention = isMention(message, serverId);
  const mentioned = mention && (await mentionNotifications());
  if (mentioned && !onScreen) markMention(key);
  const texts = noticeTexts(
    message,
    count,
    context.t,
    serverId,
    context.serverName(serverId),
    mentioned,
  );
  let outcome;
  try {
    outcome = await getApi().notifyMessage({
      serverId,
      conversation,
      onScreen,
      summary: texts.summary,
      body: texts.body,
      actionLabel: context.t("open"),
      mention,
    });
  } catch (error) {
    console.error("Deciding about a message notification failed", error);
    outcome = onScreen ? "none" : "toast";
  }
  if (outcome === "toast") {
    showToast({ key, serverId, conversation, title: texts.summary, text: texts.body });
  } else if (outcome === "none" && onScreen) {
    markSeen(key);
  }
}
