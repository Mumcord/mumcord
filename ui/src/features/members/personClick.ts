/**
 * Click behaviour shared by every place a person is shown (FR-1.62, QA.46): a single click pins
 * the profile card, a double click opens the private chat.
 */
import { getServerState } from "../channels/serverStore";
import { findPerson } from "./personLookup";
import { openPrivateConversation, type SendPrivateMessage } from "./privateMessage";
import { cancelPendingPin, closePreview, pinPreview } from "./profilePreviewStore";

/** Mouse click: the second click of a double click only cancels the pending pin. */
export function clickPerson(
  detail: number,
  session: number,
  anchor: HTMLElement,
  name?: string,
): void {
  if (detail >= 2) cancelPendingPin();
  else pinPreview(session, anchor, name, true);
}

/** Keyboard activation (Enter): pins right away, there is no second click to wait for. */
export function pinPerson(session: number, anchor: HTMLElement, name?: string): void {
  pinPreview(session, anchor, name, false);
}

/** Double click: opens the private chat. Nothing for yourself or someone who is gone. */
export function doubleClickPerson(
  session: number,
  name?: string,
  openChat: SendPrivateMessage = openPrivateConversation,
): void {
  cancelPendingPin();
  closePreview();
  const state = getServerState();
  const user = findPerson(state.users, session, name);
  if (user && user.session !== state.selfSession) openChat(user);
}
