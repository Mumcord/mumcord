import { useTranslation } from "react-i18next";
import { useIsChanged } from "./changedSettings";

/** "Changed" dot badge; renders nothing when the setting has its preset or default value. */
export function ChangedBadge({ label }: { label: string }) {
  const { t } = useTranslation("settings");
  if (!useIsChanged(label)) return null;
  return (
    <span className="setting-badge setting-badge--changed">
      <span className="setting-badge__dot" aria-hidden="true" />
      {t("badges.changed")}
    </span>
  );
}

/** Green "Recommended" pill next to the recommended choice (#159). */
export function RecommendedBadge({ id, className }: { id?: string; className?: string }) {
  const { t } = useTranslation("settings");
  return (
    <span
      id={id}
      className={`setting-badge setting-badge--recommended${className === undefined ? "" : ` ${className}`}`}
    >
      {t("badges.recommended")}
    </span>
  );
}
