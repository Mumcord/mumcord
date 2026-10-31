import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toEngineError, type MessageRequest, type User } from "../../api";
import { useServerStateOf } from "../channels";
import { setFriend, useFriend } from "../friends";
import { Avatar } from "../members/Avatar";
import { openPrivateConversation } from "../members/privateMessage";
import type { ConversationLog } from "../chat/chatStore";
import { MessageList } from "../chat/MessageList";
import { displayName } from "../servers/format";
import { useServers } from "../servers/store";
import { relativeTime } from "./relativeTime";
import { acceptRequest, declineRequest } from "./requestsStore";

/** The held messages never page or sync: they are all there is until the request is accepted. */
function heldLog(request: MessageRequest): ConversationLog {
  return {
    messages: request.messages,
    loaded: true,
    loading: false,
    hasMore: false,
    loadFailed: false,
    syncing: false,
    syncResult: null,
    syncFailed: false,
  };
}

const noop = () => undefined;

/** How often "2 min ago" is brought up to date. */
const CLOCK_MS = 30_000;

function useNow(): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, CLOCK_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);
  return now;
}

/** One held request with Accept / Decline (issue #141); a message request also offers Add friend. */
export function RequestCard({ serverId, request }: { serverId: number; request: MessageRequest }) {
  const { t, i18n } = useTranslation("chat");
  const { users } = useServerStateOf(serverId);
  const { servers } = useServers();
  const friend = useFriend(request.certHash);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const log = useMemo(() => heldLog(request), [request]);
  // Sessions are reused after someone leaves; only the same name is still the sender.
  const online = users.get(request.session);
  const sender = online?.name === request.name ? online : undefined;
  const saved = servers.find((s) => s.id === serverId);
  const serverName = saved ? displayName(saved) : "";
  const time = relativeTime(request.receivedAt, useNow(), i18n.language);
  const name = request.name;

  const run = (action: () => Promise<unknown>, opensChat: boolean) => {
    setBusy(true);
    setFailed(false);
    action().then(
      () => {
        // Accepted messages arrive in the private chat; show it while the sender is still here.
        if (opensChat && sender) openPrivateConversation(sender);
      },
      (error: unknown) => {
        console.error("Answering a message request failed", toEngineError(error).code);
        setBusy(false);
        setFailed(true);
      },
    );
  };
  const accept = () => {
    run(() => acceptRequest(serverId, request.id), true);
  };
  const decline = () => {
    run(() => declineRequest(serverId, request.id), false);
  };
  const addFriend = (user: User) => {
    run(async () => {
      await setFriend(user, true, serverId);
      await acceptRequest(serverId, request.id);
    }, true);
  };

  const actions = (
    <div className="request-card__actions">
      <button
        type="button"
        className="button button--primary"
        disabled={busy}
        data-testid="request-accept"
        onClick={accept}
      >
        {t("requests.accept")}
      </button>
      <button
        type="button"
        className="button"
        disabled={busy}
        data-testid="request-decline"
        onClick={decline}
      >
        {t("requests.decline")}
      </button>
      {!request.chatRequest && sender?.certHash != null && friend === undefined && (
        <button
          type="button"
          className="button"
          disabled={busy}
          data-testid="request-add-friend"
          onClick={() => {
            addFriend(sender);
          }}
        >
          {t("requests.addFriend")}
        </button>
      )}
    </div>
  );
  const error = failed && (
    <p role="alert" className="request-card__error">
      {t("requests.failed")}
    </p>
  );

  if (request.chatRequest) {
    return (
      <section className="request-card" aria-label={t("requests.wantsToChat", { name })}>
        <Avatar name={name} size="lg" />
        <h3 className="request-card__title">{t("requests.wantsToChat", { name })}</h3>
        <p className="request-card__meta">
          {t("requests.metaMumcord", { server: serverName, time })}
        </p>
        {request.note !== null && request.note !== "" && (
          <figure className="request-card__note">
            <figcaption>{t("requests.noteFrom", { name })}</figcaption>
            {/* Untrusted plain text: rendered as text, never as HTML. */}
            <p data-testid="request-note">{request.note}</p>
          </figure>
        )}
        <p className="request-card__explain">{t("requests.explain", { name })}</p>
        {actions}
        {error}
      </section>
    );
  }

  return (
    <section
      className="request-card request-card--messages"
      aria-label={t("requests.sentMessages", { name, count: request.messages.length })}
    >
      <h3 className="request-card__title">
        {t("requests.sentMessages", { name, count: request.messages.length })}
      </h3>
      <p className="request-card__meta">
        {t("requests.metaStandard", { server: serverName, time })}
      </p>
      <div className="request-card__messages">
        <MessageList
          log={log}
          label={t("requests.messagesLabel", { name })}
          onLoadOlder={noop}
          listKey={`request:${String(serverId)}:${String(request.id)}`}
        />
      </div>
      {actions}
      {error}
    </section>
  );
}
