/**
 * Blocking users (issue #140). `BlockDialogHost` renders the "Block …?" confirmation that
 * `openBlockDialog` opens (person menu, private messages overview, requests, notification
 * centre); `useBlock` / `isBlocked` tell whether the identity used on a server blocked someone.
 */
export { BlockDialogHost } from "./BlockDialog";
export { blockTargetFor, closeBlockDialog, openBlockDialog } from "./blockDialogStore";
export { BlockedBadge } from "./BlockedBadge";
export { BlockIcon } from "./BlockIcon";
export { CollapsedMessage } from "./CollapsedMessage";
export {
  findBlock,
  isBlocked,
  noteBlockedMessages,
  resetBlockedStore,
  subscribeBlocked,
  unblock,
  useBlock,
  useBlockedMessages,
  useBlockedUsers,
  useIsBlocked,
} from "./blockedStore";
