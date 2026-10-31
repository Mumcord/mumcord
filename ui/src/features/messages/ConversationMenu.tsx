import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { ContextMenu, type MenuItem, type MenuPosition } from "../../components/ContextMenu";
import { useServers as useConnectedServers } from "../channels";
import { clearConversation } from "../chat";
import { forgetDirectChat } from "../follow/followStore";
import { pinPreview } from "../members/profilePreviewStore";
import { openRevokeConsentDialog, useAcceptedConsent } from "../requests";
import { blockTargetFor, openBlockDialog } from "../blocking";
import { ConfirmDialog } from "../servers";
import { forgetConversation } from "./conversationsStore";
import {
  BELL_ICON,
  BELL_OFF_ICON,
  BLOCK_ICON,
  CHECK_ICON,
  PERSON_ICON,
  PIN_ICON,
  REVOKE_ICON,
  TRASH_ICON,
} from "./icons";
import type { Row } from "./overviewModel";
import { person } from "../follow/directEntries";

/** Mute lengths offered, in minutes; `null` until turned back on. */
const MUTE_CHOICES = [
  { minutes: 60, key: "overview.menu.muteHour" },
  { minutes: 8 * 60, key: "overview.menu.muteEightHours" },
  { minutes: null, key: "overview.menu.muteForever" },
] as const;

export interface ConversationMenuProps {
  row: Row;
  menu: MenuPosition;
  onClose: () => void;
  /** An action failed; the overview says so. */
  onError: () => void;
}

/**
 * Right-click (Menu key, Shift+F10) menu of a conversation: mark as read, pin, mute (a second
 * menu with the lengths), view profile, revoke chat consent, block (issue #140) and delete history on this device after
 * a confirmation.
 */
export function ConversationMenu({ row, menu, onClose, onError }: ConversationMenuProps) {
  const { t } = useTranslation("chat");
  const { t: tMembers } = useTranslation("members");
  const connected = useConnectedServers();
  // Choosing an item closes the menu first, then may open the next step in the same batch.
  const [step, setStep] = useState<"menu" | "mute" | "delete" | "closed">("menu");
  const { serverId, conversation, certHash } = row;
  const close = () => {
    setStep("closed");
  };
  useEffect(() => {
    if (step === "closed") onClose();
  }, [step, onClose]);

  const run = (action: () => Promise<unknown>, what: string) => {
    action().catch((error: unknown) => {
      console.error(`${what} failed`, error);
      onError();
    });
  };

  const markRead: MenuItem = {
    label: t("overview.menu.markRead"),
    icon: CHECK_ICON,
    disabled: row.unread === 0,
    onSelect: () => {
      run(() => getApi().conversationMarkRead(serverId, conversation), "Marking as read");
    },
  };
  // Pin and mute are saved by certificate; a chat bound to this session has none.
  const pin: MenuItem = {
    label: row.pinned ? t("overview.menu.unpin") : t("overview.menu.pin"),
    icon: PIN_ICON,
    disabled: certHash === null,
    onSelect: () => {
      if (certHash !== null) {
        run(() => getApi().pmConversationSetPinned(serverId, certHash, !row.pinned), "Pinning");
      }
    },
  };
  const mute: MenuItem = row.muted
    ? {
        label: t("overview.menu.unmute"),
        icon: BELL_ICON,
        onSelect: () => {
          if (certHash !== null) {
            run(() => getApi().pmConversationUnmute(serverId, certHash), "Unmuting");
          }
        },
      }
    : {
        label: t("overview.menu.mute"),
        icon: BELL_OFF_ICON,
        disabled: certHash === null,
        submenu: true,
        onSelect: () => {
          setStep("mute");
        },
      };
  const profile: MenuItem = {
    label: t("overview.menu.profile"),
    icon: PERSON_ICON,
    onSelect: () => {
      const users = connected.get(serverId)?.users;
      const session = users ? (person(users, conversation)?.session ?? null) : null;
      if (menu.opener) pinPreview(session, menu.opener, row.name, false, serverId);
    },
  };
  // Only a conversation the user accepted (issue #141) has a consent to take back.
  const consent = useAcceptedConsent(serverId, certHash);
  const revoke: MenuItem | null = consent
    ? {
        label: tMembers("menu.revokeConsent"),
        icon: REVOKE_ICON,
        onSelect: () => {
          openRevokeConsentDialog({ conversation: consent, opener: menu.opener });
        },
      }
    : null;
  // By certificate even while they are offline; a chat bound to a session needs them online.
  const blockTarget = blockTargetFor(
    serverId,
    row.name,
    certHash,
    certHash === null && conversation.kind === "privateSession" && row.online
      ? conversation.session
      : null,
    menu.opener,
  );
  const block: MenuItem = {
    label: t("overview.menu.block", { name: row.name }),
    icon: BLOCK_ICON,
    danger: true,
    disabled: blockTarget === null,
    onSelect: () => {
      if (blockTarget) openBlockDialog(blockTarget);
    },
  };
  const deleteHistory: MenuItem = {
    label: t("overview.menu.deleteHistory"),
    hint: t("overview.menu.deleteHint", { name: row.name }),
    icon: TRASH_ICON,
    danger: true,
    onSelect: () => {
      setStep("delete");
    },
  };

  if (step === "mute") {
    return (
      <ContextMenu
        key="mute"
        menu={menu}
        label={t("overview.menu.mute")}
        items={MUTE_CHOICES.map((choice) => ({
          label: t(choice.key),
          onSelect: () => {
            if (certHash !== null) {
              run(() => getApi().pmConversationMute(serverId, certHash, choice.minutes), "Muting");
            }
          },
        }))}
        onClose={close}
      />
    );
  }

  if (step === "delete") {
    return (
      <ConfirmDialog
        title={t("overview.deleteTitle", { name: row.name })}
        message={t("overview.deleteMessage", { name: row.name })}
        confirmLabel={t("overview.menu.deleteHistory")}
        danger
        onConfirm={() => {
          run(async () => {
            if (conversation.kind !== "privateSession") {
              await getApi().historyClear({ kind: "conversation", serverId, conversation });
            }
            clearConversation(conversation, serverId);
            forgetConversation(serverId, conversation);
            forgetDirectChat(conversation, serverId);
          }, "Deleting history");
          close();
        }}
        onClose={close}
      />
    );
  }

  return (
    <ContextMenu
      menu={menu}
      label={t("overview.menu.label", { name: row.name })}
      header={
        <div className="pm-menu__header" aria-hidden="true">
          <span className="pm-menu__name">{row.name}</span>
          <span className="pm-menu__meta">
            {[row.serverName, row.online ? t("overview.online") : t("overview.offline")]
              .filter((s) => s !== "")
              .join(" · ")}
          </span>
        </div>
      }
      items={[markRead, pin, mute, profile, ...(revoke ? [revoke] : []), block, deleteHistory]}
      onClose={close}
    />
  );
}
