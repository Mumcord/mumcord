/**
 * "BOT" pill next to a bot's name (issue #458). It takes the place of the Mumcord badge, so people
 * tell bots from people at a glance; the tester bot sends no bot marker and keeps the Mumcord badge.
 */
import { useT } from "../../i18n/useT";

export function BotBadge() {
  const t = useT("channels");
  const label = t("botBadge");
  // The leading space keeps the badge a separate word in the accessible name, as with the
  // Mumcord badge.
  return (
    <>
      {" "}
      <span className="bot-badge" data-tooltip={label} role="img" aria-label={label}>
        <svg
          viewBox="0 0 24 24"
          width="11"
          height="11"
          aria-hidden="true"
          focusable="false"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.4"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect x="4" y="8" width="16" height="12" rx="3" />
          <path d="M12 4v4M9 13v1M15 13v1" />
        </svg>
        <span aria-hidden="true">{label}</span>
      </span>
    </>
  );
}
