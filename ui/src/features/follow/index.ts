/**
 * Follow feature (B22): follow toggle, unread badges, private chat list, clear history.
 * `FollowHeaderActions` goes into `ChatView.renderHeaderActions`, `ChannelUnreadBadge` into
 * `ChannelsPanel.channelBadge`, `DirectMessages` below the channel tree.
 */
export { HeaderActions as FollowHeaderActions } from "./HeaderActions";
export { ChannelUnreadBadge } from "./UnreadBadge";
export { DirectMessages } from "./DirectMessages";
export { useFollowState, resetFollowStore } from "./followStore";
