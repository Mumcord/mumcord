/**
 * Follow feature (B22): follow toggle, unread badges, message notifications, clear history.
 * `useFollowControl` and `FollowToggle` (the header eye) are used by the channel header (#170).
 * `FollowHeaderActions` goes into `ChatView.renderHeaderActions`, `ChannelUnreadBadge` into
 * `ChannelsPanel.channelBadge`, `MessageNotifications` mounted once in the channel column. QA.13: `FollowStar` into
 * `ChannelsPanel.channelAction`, `FollowAllMenu` into `ChannelsPanel.header` and
 * `FollowNoticeLine` below the channel tree.
 */
export { HeaderActions as FollowHeaderActions } from "./HeaderActions";
export { ChannelUnreadBadge } from "./UnreadBadge";
export { MessageNotifications } from "./MessageNotifications";
export { FollowStar } from "./FollowStar";
export { FollowAllMenu, FollowNoticeLine } from "./FollowAllMenu";
export { useFollowState, resetFollowStore } from "./followStore";
export { FollowToggle } from "./FollowToggle";
export { useFollowControl, type FollowControl } from "./useFollowControl";
