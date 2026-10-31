import { useTranslation } from "react-i18next";
import { useIsMacos } from "../../api/useIsMacos";
import { unreadCount } from "./centreModel";
import { toggleCentre, useCentreOpen } from "./centreStore";
import { CENTRE_PANEL_ID } from "./centreShortcut";
import { useCentreItems } from "./useCentreItems";

const BELL_PATH = "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20h4";

/**
 * The bell in the chat header (#143, FR-3.105): opens the notification centre; the badge counts
 * unread entries across all servers, also during Do Not Disturb (Q1.5).
 */
export function NotificationBell() {
  const { t } = useTranslation("notifications");
  const macos = useIsMacos();
  const open = useCentreOpen();
  const unread = unreadCount(useCentreItems());
  const action = t("centre.title");
  return (
    <button
      type="button"
      className="centre-bell"
      data-centre-bell=""
      data-testid="notification-bell"
      aria-label={unread > 0 ? t("centre.bellUnread", { count: unread }) : action}
      aria-expanded={open}
      aria-controls={open ? CENTRE_PANEL_ID : undefined}
      aria-haspopup="dialog"
      aria-keyshortcuts="Control+Shift+N Meta+Shift+N"
      data-tooltip={action}
      data-tooltip-placement="bottom"
      data-tooltip-shortcut={macos ? "⌘⇧N" : t("centre.shortcut")}
      onClick={toggleCentre}
    >
      <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" focusable="false">
        <path
          d={BELL_PATH}
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      {unread > 0 && (
        <span className="centre-bell__badge" aria-hidden="true" data-testid="notification-badge">
          {unread > 99 ? "99+" : unread}
        </span>
      )}
    </button>
  );
}
