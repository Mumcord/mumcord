/**
 * Friends feature (FR-1.64, FR-1.65): friends by certificate hash with their online state.
 * `FriendsList` goes below the channel tree; `useFriend(certHash)` and `setFriend(user, on)` back
 * the friend switches in the person menu, the pinned card and the full profile.
 */
export { FriendsList } from "./FriendsList";
export { PrivatePresence } from "./PrivatePresence";
export { resetFriendsStore, useFriend, useFriends } from "./friendsStore";
export { setFriend } from "./friendToggle";
