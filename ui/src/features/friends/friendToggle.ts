import { getApi, type User } from "../../api";

/** Adds or removes `user` as a friend (FR-1.64); people without a certificate cannot be friends. */
export function setFriend(user: User, friend: boolean): Promise<null> {
  if (user.certHash === null) return Promise.reject(new Error("no certificate"));
  return friend ? getApi().friendAdd(user.session) : getApi().friendRemove(user.certHash);
}
