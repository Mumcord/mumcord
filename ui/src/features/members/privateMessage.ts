/**
 * "Send private message" integration point. By default it opens the private conversation in the
 * chat column via the shared selection; B22 can pass its own `onSendPrivateMessage` to the
 * panels instead (e.g. to also focus the chat input).
 */
import type { Conversation, User } from "../../api";
import { selectConversation } from "../channels/selection";

export type SendPrivateMessage = (user: User) => void;

/** People with a certificate keep their chat across sessions; others only for this session. */
export function privateConversation(user: User): Conversation {
  return user.certHash === null
    ? { kind: "privateSession", session: user.session }
    : { kind: "private", certHash: user.certHash };
}

export const openPrivateConversation: SendPrivateMessage = (user) => {
  selectConversation(privateConversation(user));
};
