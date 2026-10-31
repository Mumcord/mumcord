/**
 * Channels feature (B20) and the shared server state used by other features.
 *
 * Stable contracts for other features:
 * - Several servers can be connected (FR-2.1). The **shown server** is the one whose channels,
 *   chat and members the window shows (`useShownServerId()`, `getShownServerId()`,
 *   `showServer(id)`, `shownServerId()` for commands); the **voice server** the one with voice
 *   (`useVoiceServerId()`, `getVoiceServerId()`, `voiceServerId()` for commands).
 * - `useServerState()`: live connection (`connection`, `connected`), server info incl. limits
 *   (`info`), channels, users and talking sessions of the shown server (maps keyed by channel
 *   id / session), own session (`selfSession`, `selfUser(state)`). Empty (`serverId === null`)
 *   when not connected or not yet synced; kept during reconnecting.
 * - `useServers()`: every connected server's state by server id; `useServerStateOf(id)` and
 *   `getServerStateOf(id)` for one of them.
 * - `useServerSelector(select, equal?)`: one derived value; re-renders only when it changes. Use it
 *   in components rendered many times (e.g. per tree row) instead of `useServerState()`.
 * - `subscribeServerChanges(fn)`: semantic changes of the shown server (synced, connection,
 *   channels, user joined/moved/left, talk, and `shown` when another server is shown) for side
 *   effects such as sounds or announcements; `subscribeAllServerChanges(fn)` for every server.
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
  getServerStateOf,
  getShownServerId,
  getVoiceServerId,
  selfUser,
  showServer,
  shownServerId,
  subscribeAllServerChanges,
  subscribeServerChanges,
  useServerSelector,
  useServers,
  useServerState,
  useServerStateOf,
  useShownServerId,
  useVoiceServerId,
  voiceServerId,
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
export type { ChannelAction, ChannelMenuRequest } from "./ChannelTree";
export { ProfileCard, type ProfileCardProps } from "../members/ProfileCard";
