/**
 * Channels feature (B20) and the shared server state used by other features.
 *
 * Stable contracts for other features:
 * - `useServerState()`: live connection (`connection`, `connected`), server info incl. limits
 *   (`info`), channels, users and talking sessions of the connected server (maps keyed by channel
 *   id / session), own session (`selfSession`, `selfUser(state)`). Empty (`serverId === null`)
 *   when not connected or not yet synced; kept during reconnecting.
 * - `useServerSelector(select, equal?)`: one derived value; re-renders only when it changes. Use it
 *   in components rendered many times (e.g. per tree row) instead of `useServerState()`.
 * - `subscribeServerChanges(fn)`: semantic changes (synced, connection, channels, user joined/moved/left,
 *   talk) for side effects such as sounds or announcements.
 * - `useSelectedConversation()`: `{ conversation, select }` – the conversation the chat column
 *   shows. Single click on a channel selects it; "Send private message" selects a private one.
 *   Starts at the own channel after each sync, `null` when disconnected.
 * - `ChannelsPanel` props: `channelBadge(channelId)` slot for the unread badge (B22),
 *   `channelAction({ channelId, name, tabbable })` fixed slot for a row button such as the follow
 *   star (QA.13), `header` above the tree, and `onSendPrivateMessage(user)` to override the default "open private chat" behaviour.
 * - `ProfileCard` (`{ session, onClose, returnFocus?, onSendPrivateMessage? }`): modal profile
 *   card of one person.
 */
export {
  getServerState,
  selfUser,
  subscribeServerChanges,
  useServerSelector,
  useServerState,
  type ServerChange,
  type ServerState,
} from "./serverStore";
export {
  getSelectedConversation,
  sameConversation,
  selectConversation,
  useSelectedConversation,
} from "./selection";
export { ChannelsPanel, type ChannelsPanelProps } from "./ChannelsPanel";
export type { ChannelAction } from "./ChannelTree";
export { ProfileCard, type ProfileCardProps } from "../members/ProfileCard";
