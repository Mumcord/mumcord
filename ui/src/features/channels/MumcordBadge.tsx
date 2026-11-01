/**
 * Small Mumcord logo next to the name of people who use Mumcord too (FR-1.61, QA.45). The engine
 * recognises them by their plugin-data hello; official clients never get the badge.
 */
import { useT } from "../../i18n/useT";
import type { User } from "../../api";
import { isMumcord } from "./mumcord";

/**
 * The app icon in miniature: the two-people "M" with the cap and the beanie on its violet tile,
 * which keeps the same contrast on light and dark themes.
 */
export function MumcordBadge({
  user,
  size = 13,
}: {
  user: Pick<User, "isMumcord"> | undefined;
  size?: number;
}) {
  const t = useT("channels");
  if (!isMumcord(user)) return null;
  const label = t("mumcordBadge");
  // The leading space keeps the badge a separate word in the accessible name: inline elements
  // join without one ("MiraUses Mumcord"), and a space inside a child element is trimmed away.
  return (
    <>
      {" "}
      <span className="mumcord-badge" data-tooltip={label} role="img" aria-label={label}>
        <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false">
          <rect width="24" height="24" rx="6" fill="#2f027f" />
          <path
            d="M5.5 19.5 7.2 13.2 12 17 16.8 13.2 18.5 19.5"
            fill="none"
            stroke="#e1d0fd"
            strokeWidth="3.4"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <circle cx="7.6" cy="9.3" r="2.4" fill="#e1d0fd" />
          <circle cx="16.4" cy="9.3" r="2.4" fill="#e1d0fd" />
          <path
            d="M5 8.2C5 5.2 10.2 5.2 10.2 8.2ZM5.4 8.2 2.6 9.4"
            fill="#9a62fe"
            stroke="#9a62fe"
            strokeWidth="1"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path d="M13.8 8.2C13.8 5 19 5 19 8.2Z" fill="#fdb558" />
          <circle cx="16.4" cy="5.4" r="1.2" fill="#fb777c" />
        </svg>
      </span>
    </>
  );
}
