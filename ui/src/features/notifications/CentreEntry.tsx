import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { toEngineError } from "../../api";
import { getServerStateOf, useServerStateOf } from "../channels";
import { BlockIcon, blockTargetFor, openBlockDialog } from "../blocking";
import { Avatar } from "../members/Avatar";
import { openPrivateConversation } from "../members/privateMessage";
import { acceptRequest, declineRequest } from "../requests/requestsStore";
import { relativeTime } from "../requests/relativeTime";
import {
  joinWatchParty,
  openItem,
  restartForUpdate,
  showWhatsNew,
  watchShare,
} from "./centreActions";
import type { CentreItem, InviteItem, RequestItem, UpdateItem } from "./centreModel";
import { dismissItem, setCentreOpen } from "./centreStore";
import { usePluginIcon } from "../plugins/render/usePluginIcon";
import { PluginActions, usePluginEntryText } from "../plugins/notifications";

type T = TFunction<"notifications">;

/** Who the entry is about; untrusted plain text. Empty for connection events and updates. */
function senderOf(item: CentreItem): string {
  switch (item.kind) {
    case "request":
      return item.request.name;
    case "connection":
    case "update":
    case "plugin":
      return "";
    default:
      return item.sender;
  }
}

function titleOf(item: CentreItem, t: T, server: string): string {
  const name = senderOf(item) || t("centre.someone");
  switch (item.kind) {
    case "mention":
      return item.group ? t("centre.mentionGroup", { name }) : t("centre.mention", { name });
    case "reply":
      return t("centre.reply", { name });
    case "request":
      return item.request.chatRequest
        ? t("centre.chatRequest", { name })
        : t("centre.messageRequest", { name });
    case "invite":
      return t(`centre.invite.${item.invite}`, { name });
    case "connection":
      return t("centre.connection", { server });
    case "update":
      return t("centre.update.title", { version: item.version });
    case "plugin":
      return "";
  }
}

function bodyOf(item: CentreItem, t: T): string {
  switch (item.kind) {
    case "request":
      if (item.request.chatRequest) {
        return item.request.note ? t("centre.quoted", { text: item.request.note }) : "";
      }
      return t("centre.messageRequestBody", { count: item.request.messages.length });
    case "connection":
      return item.text;
    case "update":
      return t("centre.update.body");
    case "plugin":
      return "";
    case "invite":
      if (item.invite === "watchParty" || item.invite === "screenShare") return "";
      return item.excerpt ? t("centre.quoted", { text: item.excerpt }) : "";
    default:
      return item.excerpt;
  }
}

/** The second chip: the channel or what kind of entry it is. */
function whereOf(item: CentreItem, t: T): string {
  if (item.kind === "request") {
    return item.request.chatRequest
      ? t("centre.type.chatRequest")
      : t("centre.type.messageRequest");
  }
  if (item.kind === "connection") return t("centre.type.connection");
  if (item.kind === "update") return t("centre.type.update");
  if (item.kind === "plugin") return "";
  if (item.kind === "invite") return t("centre.type.invite");
  if (item.conversation.kind !== "channel") return t("centre.type.private");
  const channel = getServerStateOf(item.serverId).channels.get(item.conversation.channelId);
  return channel ? t("channel", { name: channel.name }) : t("unknownChannel");
}

/** Accept / Decline as on the request card (#141); Block (#140) joins them later. */
function RequestActions({ item }: { item: RequestItem }) {
  const { t } = useTranslation("notifications");
  const { users } = useServerStateOf(item.serverId);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const { request, serverId } = item;
  // Sessions are reused after someone leaves; only the same name is still the sender.
  const online = users.get(request.session);
  const sender = online?.name === request.name ? online : undefined;

  const run = (action: () => Promise<unknown>, opensChat: boolean) => {
    setBusy(true);
    setFailed(false);
    action().then(
      () => {
        // Accepted messages arrive in the private chat; show it while the sender is still here.
        if (opensChat && sender) {
          setCentreOpen(false);
          openPrivateConversation(sender, serverId);
        }
      },
      (error: unknown) => {
        console.error("Answering a message request failed", toEngineError(error).code);
        setBusy(false);
        setFailed(true);
      },
    );
  };

  return (
    <>
      <button
        type="button"
        className="button button--primary centre-entry__action"
        disabled={busy}
        data-testid="centre-accept"
        onClick={() => {
          run(() => acceptRequest(serverId, request.id), true);
        }}
      >
        {t("centre.accept")}
      </button>
      <button
        type="button"
        className="button centre-entry__action"
        disabled={busy}
        data-testid="centre-decline"
        onClick={() => {
          run(() => declineRequest(serverId, request.id), false);
        }}
      >
        {t("centre.decline")}
      </button>
      <button
        type="button"
        className="button centre-entry__action centre-entry__block"
        disabled={busy}
        data-testid="centre-block"
        onClick={(event) => {
          // Sessions are reused: only the sender still here is blocked by session.
          const target = blockTargetFor(
            serverId,
            request.name,
            request.certHash,
            sender?.session ?? null,
            event.currentTarget,
          );
          if (target) openBlockDialog(target);
        }}
      >
        <BlockIcon />
        {t("centre.block")}
      </button>
      {failed && (
        <span className="centre-entry__error" role="alert">
          {t("centre.failed")}
        </span>
      )}
    </>
  );
}

function InviteActions({ item }: { item: InviteItem }) {
  const { t } = useTranslation("notifications");
  let primary: ReactNode;
  if (item.invite === "watchParty") {
    primary = (
      <button
        type="button"
        className="button button--primary centre-entry__action"
        onClick={() => {
          joinWatchParty(item.serverId, item.id);
        }}
      >
        {t("centre.join")}
      </button>
    );
  } else if (item.invite === "screenShare") {
    primary = (
      <button
        type="button"
        className="button button--primary centre-entry__action"
        onClick={() => {
          watchShare(item.serverId, item.session, item.id);
        }}
      >
        {t("centre.watch")}
      </button>
    );
  } else {
    primary = (
      <button
        type="button"
        className="button button--primary centre-entry__action"
        onClick={() => {
          openItem(item);
        }}
      >
        {t("centre.open")}
      </button>
    );
  }
  return (
    <>
      {primary}
      <button
        type="button"
        className="button centre-entry__action"
        onClick={() => {
          dismissItem(item.id);
        }}
      >
        {t("centre.dismiss")}
      </button>
    </>
  );
}

/** "Restart to update" (same rule as the update toast) and "What's new" (#168). */
function UpdateActions({ item }: { item: UpdateItem }) {
  const { t } = useTranslation("notifications");
  return (
    <>
      <button
        type="button"
        className="button button--primary centre-entry__action"
        onClick={() => {
          restartForUpdate(item.id);
        }}
      >
        {t("centre.update.restart")}
      </button>
      <button
        type="button"
        className="button centre-entry__action"
        onClick={() => {
          showWhatsNew(item);
        }}
      >
        {t("centre.update.whatsNew")}
      </button>
    </>
  );
}

/**
 * One entry: avatar, bold title and a dot while unread, time, excerpt and chips for server and
 * channel or kind. The whole text is one button that opens the entry; answers sit below it.
 */
export function CentreEntry({
  item,
  now,
  serverName,
}: {
  item: CentreItem;
  now: number;
  serverName: string;
}) {
  const { t, i18n } = useTranslation("notifications");
  const plugin = usePluginEntryText(item.kind === "plugin" ? item : null);
  const title = item.kind === "plugin" ? plugin.title : titleOf(item, t, serverName);
  const body = item.kind === "plugin" ? plugin.body : bodyOf(item, t);
  const time = relativeTime(new Date(item.at).toISOString(), now, i18n.language);
  const pluginId = item.kind === "plugin" ? item.notice.plugin : null;
  const pluginIcon = usePluginIcon(pluginId);
  const avatarName =
    item.kind === "plugin" ? item.notice.name : senderOf(item) || serverName || "Mumcord";
  return (
    <li className="centre-entry" data-unread={!item.read || undefined} data-kind={item.kind}>
      <Avatar
        name={avatarName}
        src={item.kind === "update" ? `${import.meta.env.BASE_URL}favicon.png` : pluginIcon}
      />
      <div className="centre-entry__content">
        <button
          type="button"
          className="centre-entry__main"
          data-testid="centre-entry"
          onClick={() => {
            openItem(item);
          }}
        >
          <span className="centre-entry__head">
            <span className="centre-entry__title">{title}</span>
            <span className="centre-entry__time">{time}</span>
            {!item.read && (
              <span className="centre-entry__dot">
                <span className="visually-hidden">{t("centre.unread")}</span>
              </span>
            )}
          </span>
          {body && <span className="centre-entry__body">{body}</span>}
          <span className="centre-entry__chips">
            {serverName && <span className="centre-chip">{serverName}</span>}
            <span className="centre-chip">
              {item.kind === "plugin" ? plugin.where : whereOf(item, t)}
            </span>
          </span>
        </button>
        {item.kind === "request" && (
          <div className="centre-entry__actions">
            <RequestActions item={item} />
          </div>
        )}
        {item.kind === "invite" && (
          <div className="centre-entry__actions">
            <InviteActions item={item} />
          </div>
        )}
        {item.kind === "update" && (
          <div className="centre-entry__actions">
            <UpdateActions item={item} />
          </div>
        )}
        {item.kind === "plugin" && <PluginActions item={item} />}
      </div>
      {item.kind !== "invite" && (
        <button
          type="button"
          className="centre-entry__dismiss"
          aria-label={t("centre.dismissItem", { title })}
          data-tooltip={t("centre.dismiss")}
          onClick={() => {
            dismissItem(item.id);
          }}
        >
          ✕
        </button>
      )}
    </li>
  );
}
