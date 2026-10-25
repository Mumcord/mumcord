import { useTranslation } from "react-i18next";
import { useFriend } from "./friendsStore";
import { lastSeenText } from "./presence";
import "./friends.css";

/**
 * Whether the other person of a private chat is online (FR-1.65): a dot plus text, so colour is
 * never the only cue. Friends also show when they were last seen. `compact` keeps the text for
 * screen readers only (list rows).
 */
export function PrivatePresence({
  online,
  certHash,
  className,
  compact = false,
}: {
  online: boolean;
  certHash: string | null;
  className?: string;
  compact?: boolean;
}) {
  const { t, i18n } = useTranslation("friends");
  const friend = useFriend(certHash);
  const text = online ? t("online") : lastSeenText(t, friend?.lastSeenAt ?? null, i18n.language);
  return (
    <span className={className}>
      <span className="presence-dot" data-online={online} aria-hidden="true" />
      <span className={compact ? "visually-hidden" : undefined}>{text}</span>
    </span>
  );
}
