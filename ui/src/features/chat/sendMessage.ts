/**
 * Sending own messages (FR-2.74): each shows at once, greyed, and turns normal in place when the
 * engine confirms it. The engine only confirms after a short wait for a refusal (FR-2.47), so the
 * UI never waits for it. A failure (a refusal, a lost connection) keeps the message with the reason
 * so it can be retried or deleted; screen readers hear a polite notice then, and nothing on success.
 */
import type { TFunction } from "i18next";
import { announce } from "../../a11y/announcer";
import { getApi, type ChatMessage, type Conversation } from "../../api";
import { getServerStateOf, selfUser } from "../channels/serverStore";
import { addOutgoing, confirmOutgoing, setOutgoingError, type OutgoingSend } from "./chatStore";
import type { SendTarget } from "./conversationInfo";
import { describeSendError } from "./errors";

/** The conversation a message to `target` belongs to, as the engine files it. */
export function conversationOf(target: SendTarget, certHash: string | null): Conversation {
  if (target.kind === "channel") return { kind: "channel", channelId: target.channelId };
  return certHash === null
    ? { kind: "privateSession", session: target.session }
    : { kind: "private", certHash };
}

/**
 * Messages go out one after the other per server: they arrive in the order they were sent, and
 * the engine can tell which message a refusal belongs to.
 */
const queues = new Map<number, Promise<void>>();

function enqueue(serverId: number, job: () => Promise<void>): void {
  queues.set(serverId, (queues.get(serverId) ?? Promise.resolve()).then(job));
}

/**
 * The session to send a direct message to now: after a reconnect the person has a new session,
 * found by their certificate. `null` when they are offline.
 */
function currentTarget(entry: OutgoingSend): SendTarget | null {
  const { conversation, target } = entry;
  if (conversation.kind !== "private") return target;
  for (const user of getServerStateOf(entry.serverId).users.values()) {
    if (user.certHash === conversation.certHash) return { kind: "private", session: user.session };
  }
  return null;
}

function sendHtml(serverId: number, target: SendTarget, html: string): Promise<ChatMessage> {
  const api = getApi();
  return target.kind === "channel"
    ? api.sendChannelMessage(serverId, target.channelId, html)
    : api.sendPrivateMessage(serverId, target.session, html);
}

async function deliver(t: TFunction<"chat">, entry: OutgoingSend): Promise<void> {
  const target = currentTarget(entry);
  try {
    if (target === null) {
      setOutgoingError(entry, t("errors.personOffline"));
    } else {
      confirmOutgoing(entry, await sendHtml(entry.serverId, target, entry.html));
      return;
    }
  } catch (error) {
    setOutgoingError(entry, describeSendError(t, error, entry.tooLarge));
  }
  announce(t("failed.announce"));
}

/**
 * Shows an own message at once and sends it in the background. `tooLarge` is the text shown if
 * the server finds it too large.
 */
export function sendOwn(
  t: TFunction<"chat">,
  serverId: number,
  target: SendTarget,
  html: string,
  tooLarge: string,
): void {
  const state = getServerStateOf(serverId);
  const self = selfUser(state);
  const certHash =
    target.kind === "private" ? (state.users.get(target.session)?.certHash ?? null) : null;
  const entry = addOutgoing(
    { serverId, conversation: conversationOf(target, certHash), target, html, tooLarge },
    {
      senderName: self?.name ?? "",
      senderCertHash: self?.certHash ?? null,
      senderSession: state.selfSession,
    },
  );
  enqueue(serverId, () => deliver(t, entry));
}

/** Sends a failed message again; it shows greyed (sending) until the engine confirms it. */
export function retrySend(t: TFunction<"chat">, entry: OutgoingSend): void {
  setOutgoingError(entry, null);
  enqueue(entry.serverId, () => deliver(t, entry));
}
