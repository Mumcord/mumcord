import type { KeyboardEvent, MouseEvent, Ref } from "react";
import { useTranslation } from "react-i18next";
import { Avatar } from "../members/Avatar";
import { initials } from "../servers/format";
import { BELL_OFF_ICON, PIN_ICON, Icon } from "./icons";
import type { Row } from "./overviewModel";

export interface ConversationRowProps {
  row: Row;
  /** Time text of the newest message, empty without one. */
  time: string;
  /** The conversation open in the chat column. */
  current: boolean;
  /** The row the arrow keys are on: the only one in the tab order. */
  active: boolean;
  buttonRef: Ref<HTMLButtonElement>;
  onOpen: () => void;
  onFocus: () => void;
  onMenu: (event: MouseEvent<HTMLButtonElement> | KeyboardEvent<HTMLButtonElement>) => void;
  /** Arrow keys, Home and End move between the rows. */
  onNavigate: (event: KeyboardEvent<HTMLButtonElement>) => void;
}

/** One conversation: avatar with online state, name, server tag, preview, time, unread count. */
export function ConversationRow({
  row,
  time,
  current,
  active,
  buttonRef,
  onOpen,
  onFocus,
  onMenu,
  onNavigate,
}: ConversationRowProps) {
  const { t } = useTranslation("chat");
  const last = row.last;
  const text = last === null ? "" : last.text || (last.image ? t("overview.image") : "");
  const preview = last?.own === true && text !== "" ? t("overview.you", { text }) : text;
  const unread = row.unread > 99 ? "99+" : String(row.unread);
  const label = [
    row.name,
    row.serverName,
    row.online ? t("overview.online") : t("overview.offline"),
    row.unread > 0 ? t("overview.unreadCount", { count: row.unread }) : null,
    row.pinned ? t("overview.pinned") : null,
    row.muted ? t("overview.muted") : null,
    preview,
    time,
  ]
    .filter((part) => part !== null && part !== "")
    .join(", ");

  return (
    <button
      ref={buttonRef}
      type="button"
      className="pm-row"
      data-unread={row.unread > 0 || undefined}
      data-muted={row.muted || undefined}
      data-testid="pm-row"
      tabIndex={active ? 0 : -1}
      aria-label={label}
      aria-current={current || undefined}
      onClick={onOpen}
      onFocus={onFocus}
      onContextMenu={(event) => {
        event.preventDefault();
        onMenu(event);
      }}
      onKeyDown={(event) => {
        if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
          event.preventDefault();
          onMenu(event);
        } else {
          onNavigate(event);
        }
      }}
    >
      <span className="pm-row__avatar" aria-hidden="true">
        <Avatar name={row.name} />
        <span className="pm-row__presence" data-online={row.online} />
      </span>
      <span className="pm-row__main" aria-hidden="true">
        <span className="pm-row__top">
          <span className="pm-row__name">{row.name}</span>
          {row.serverName !== "" && (
            <span className="pm-row__server" data-tooltip={row.serverName}>
              {initials(row.serverName)}
            </span>
          )}
          {row.pinned && <Icon path={PIN_ICON} className="pm-row__mark" />}
          {row.muted && <Icon path={BELL_OFF_ICON} className="pm-row__mark" />}
          <span className="pm-row__time">{time}</span>
        </span>
        <span className="pm-row__bottom">
          <span className="pm-row__preview">{preview}</span>
          {row.unread > 0 && <span className="pm-row__count">{unread}</span>}
        </span>
      </span>
    </button>
  );
}
