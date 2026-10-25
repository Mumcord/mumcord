/**
 * Small Mumcord logo next to the name of people who use Mumcord too (FR-1.61, QA.45). The engine
 * recognises them by their plugin-data hello; official clients never get the badge.
 */
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { isMumcord } from "./mumcord";

/** The two-people "M" of the logo, reduced to two brand-coloured shapes. */
export function MumcordBadge({
  user,
  size = 13,
}: {
  user: Pick<User, "isMumcord"> | undefined;
  size?: number;
}) {
  const { t } = useTranslation("channels");
  if (!isMumcord(user)) return null;
  const label = t("mumcordBadge");
  return (
    <span className="mumcord-badge" title={label} role="img" aria-label={label}>
      <svg viewBox="0 0 24 24" width={size} height={size} aria-hidden="true" focusable="false">
        <circle cx="7" cy="4.8" r="3" fill="var(--color-mumcord-a, #ff8a9b)" />
        <circle cx="17" cy="4.8" r="3" fill="var(--color-mumcord-b, #9b7cff)" />
        <path
          d="M2 21.500V12.500C2 9.800 3.800 8.500 6 8.500C8.500 8.500 10.200 10.200 12 12V21.500Z"
          fill="var(--color-mumcord-a, #ff8a9b)"
        />
        <path
          d="M22 21.500V12.500C22 9.800 20.200 8.500 18 8.500C15.500 8.500 13.800 10.200 12 12V21.500Z"
          fill="var(--color-mumcord-b, #9b7cff)"
        />
      </svg>
    </span>
  );
}
