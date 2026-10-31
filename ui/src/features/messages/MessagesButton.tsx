import { useTranslation } from "react-i18next";
import { useAllPmRequests } from "../requests";
import { CHAT_ICON, Icon } from "./icons";
import { openMessages, useMessagesOpen } from "./messagesView";
import { useOverviewRows } from "./useOverviewRows";
import "./messages.css";

/**
 * "Messages" at the top of the server bar: opens the private messages overview. The badge counts
 * unread messages of conversations that are not muted, plus message requests. `covered` is set while
 * another page (the public server browser) hides the overview; `onOpen` lets that page close.
 */
export function MessagesButton({
  covered = false,
  onOpen,
}: { covered?: boolean; onOpen?: () => void } = {}) {
  const { t } = useTranslation("chat");
  const open = useMessagesOpen() && !covered;
  const rows = useOverviewRows();
  const requests = useAllPmRequests();
  const count =
    rows.reduce((sum, row) => sum + (row.muted ? 0 : row.unread), 0) +
    [...requests.values()].reduce((sum, r) => sum + r.incoming.length, 0);
  const label = count > 0 ? t("overview.buttonUnread", { count }) : t("overview.title");
  return (
    <div className="server-bar__item pm-button" data-shown={open || undefined}>
      <button
        type="button"
        className="server-bar__server pm-button__button"
        aria-label={label}
        aria-current={open ? "true" : undefined}
        data-tooltip={label}
        data-tooltip-placement="right"
        data-testid="pm-button"
        onClick={() => {
          onOpen?.();
          openMessages();
        }}
      >
        <Icon path={CHAT_ICON} />
        {count > 0 && (
          <span className="server-bar__mark server-bar__mark--mentions" aria-hidden="true">
            {count > 99 ? "99+" : count}
          </span>
        )}
      </button>
    </div>
  );
}
