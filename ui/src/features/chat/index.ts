/**
 * Chat feature (B21). Stable contracts for other features:
 * - `<ChatView renderHeaderActions={(conversation) => …} />`: slot in the chat header for the
 *   Follow toggle (B22). Sender names open B20's profile card. `renderChannelTitle(channelId)`
 *   replaces a channel's title with the channel-name menu (#170).
 * - Message store (one per app, cleared on every `serverSynced` and on disconnect):
 *   `useConversationMessages(conversation)` / `useConversationLog(conversation)` (messages plus
 *   loading state), `loadOlder(conversation)`, `clearConversation(conversation)` (call after
 *   clearing its history), `subscribeIncoming(fn)` for every incoming message.
 * - `sanitizeHtml(html)` for any other server-provided HTML.
 */
export { ChatView, type ChatViewProps } from "./ChatView";
export {
  clearConversation,
  conversationKey,
  loadOlder,
  subscribeIncoming,
  useConversationLog,
  useConversationMessages,
  type ConversationLog,
} from "./chatStore";
export { sanitizeHtml } from "./sanitize";
export { isMention } from "./ownMention";
