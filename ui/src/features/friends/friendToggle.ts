import { getApi, type User } from "../../api";
import { shownServerId } from "../channels";

/** Adds or removes `user` as a friend (FR-1.64); people without a certificate cannot be friends. */
export function setFriend(user: User, friend: boolean, serverId?: number): Promise<null> {
  if (user.certHash === null) return Promise.reject(new Error("no certificate"));
  return friend
    ? getApi().friendAdd(serverId ?? shownServerId(), user.session)
    : getApi().friendRemove(user.certHash);
}
