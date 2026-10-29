import { useTranslation } from "react-i18next";
import type { AudioIssue, AudioRating } from "../../bindings/commands";
import "./audioHealth.css";

/** Each band has its own shape, so the rating never depends on colour alone. */
const ICONS: Record<AudioRating, string> = {
  good: "M5 12.5l4.5 4.5L19 7.5",
  fair: "M12 3.5L2.5 20h19L12 3.5zM12 10v4.5M12 17.5v.01",
  poor: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM9 9l6 6M15 9l-6 6",
};

function RatingIcon({ rating, size }: { rating: AudioRating; size: number }) {
  return (
    <svg
      className="audio-rating__icon"
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
    >
      <path
        d={ICONS[rating]}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** The one-line cause of a Fair or Poor rating; `null` while it is good. */
function useRatingReason(issue: AudioIssue | null): string | null {
  const { t } = useTranslation("connection");
  return issue === null ? null : t(`reason.${issue}`);
}

/**
 * Always-visible verdict on the local audio path: "Audio: Fair" with an icon and the most likely
 * cause, or how long it has been fine.
 */
export function AudioHealthRating(props: {
  rating: AudioRating;
  issue: AudioIssue | null;
  minutes: number;
}) {
  const { t } = useTranslation("connection");
  const issueReason = useRatingReason(props.issue);
  const reason =
    props.rating === "good" || issueReason === null
      ? t("reason.good", { count: props.minutes })
      : issueReason;
  return (
    <div className="audio-rating" data-rating={props.rating}>
      <RatingIcon rating={props.rating} size={20} />
      <div>
        <p className="audio-rating__title">
          {t("audioRating", { rating: t(`rating.${props.rating}`) })}
        </p>
        <p className="audio-rating__reason">{reason}</p>
      </div>
    </div>
  );
}

/** Compact rating for tight places such as the packet inspector's voice counter line. */
export function AudioRatingBadge({
  rating,
  issue,
}: {
  rating: AudioRating;
  issue: AudioIssue | null;
}) {
  const { t } = useTranslation("connection");
  const reason = useRatingReason(issue);
  return (
    <span className="audio-badge-group">
      <span className="audio-badge" data-rating={rating}>
        <RatingIcon rating={rating} size={14} />
        {/* Screen readers hear "Audio: Fair"; sighted users get the context from the line. */}
        <span className="visually-hidden">
          {t("audioRating", { rating: t(`rating.${rating}`) })}
        </span>
        <span aria-hidden="true">{t(`rating.${rating}`)}</span>
      </span>
      {reason !== null && <span className="audio-badge__reason">{reason}</span>}
    </span>
  );
}
