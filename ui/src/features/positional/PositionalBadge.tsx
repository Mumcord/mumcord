import { useTranslation } from "react-i18next";
import { usePositionalSending } from "./positionalStore";
import "./PositionalBadge.css";

/**
 * Small "3D" status icon next to a person who currently sends positions (FR-1.72). "3D" reads
 * the same in every language, so only the tooltip and the spoken label are translated.
 */
export function PositionalBadge({ session }: { session: number }) {
  const { t } = useTranslation("channels");
  const sending = usePositionalSending(session);
  if (!sending) return null;
  const tip = t("statusTip.positional");
  return (
    <span className="positional-badge" title={tip} role="img" aria-label={tip}>
      <span aria-hidden="true">3D</span>
    </span>
  );
}
