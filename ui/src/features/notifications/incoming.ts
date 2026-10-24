/**
 * Turns one incoming message into a toast or system notification (FR-1.48c). The engine decides
 * by window focus and setting (`notifyMessage`); this side knows which chat is on screen and
 * builds the translated, plain-text texts.
 */
import type { TFunction } from "i18next";
import { getApi, type ChatMessage, type Conversation } from "../../api";
import { getSelectedConversation, getServerState, sameConversation } from "../channels";
import { conversationKey } from "../chat";
import { bumpCount, markSeen, showToast } from "./notificationStore";
import { hasImage, plainPreview } from "./preview";

export interface IncomingContext {
  t: TFunction<"notifications">;
  /** A full-window overlay (settings) covers the chat. */
  overlayOpen: boolean;
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
): NoticeTexts {
  const { conversation } = message;
  const isChannel = conversation.kind === "channel";
  const name = isChannel ? channelLabel(conversation.channelId, t) : message.senderName;
  const summary =
    count === 1 ? name : t(isChannel ? "groupedChannel" : "groupedPrivate", { count, name });
  const preview = plainPreview(message.body) || (hasImage(message.body) ? t("image") : "");
  const body = isChannel ? t("line", { sender: message.senderName, preview }) : preview;
  return { summary, body };
}

function channelLabel(channelId: number, t: TFunction<"notifications">): string {
  const channel = getServerState().channels.get(channelId);
  return channel ? t("channel", { name: channel.name }) : t("unknownChannel");
}

/** True when the UI shows this chat right now (window focus is checked by the engine). */
function isOnScreen(conversation: Conversation, overlayOpen: boolean): boolean {
  return !overlayOpen && sameConversation(getSelectedConversation(), conversation);
}

export async function handleIncoming(
  message: ChatMessage,
  context: IncomingContext,
): Promise<void> {
  if (message.own) return;
  const { conversation } = message;
  const key = conversationKey(conversation);
  const onScreen = isOnScreen(conversation, context.overlayOpen);
  const count = bumpCount(key);
  const texts = noticeTexts(message, count, context.t);
  let outcome;
  try {
    outcome = await getApi().notifyMessage({
      conversation,
      onScreen,
      summary: texts.summary,
      body: texts.body,
      actionLabel: context.t("open"),
    });
  } catch (error) {
    console.error("Deciding about a message notification failed", error);
    outcome = onScreen ? "none" : "toast";
  }
  if (outcome === "toast") {
    showToast({ key, conversation, title: texts.summary, text: texts.body });
  } else if (outcome === "none" && onScreen) {
    markSeen(key);
  }
}
