import { useT } from "../../i18n/useT";
import { usePositionalSending } from "./positionalStore";
import "./PositionalBadge.css";

/**
 * Small "3D" status icon next to a person who currently sends positions (FR-1.72). "3D" reads
 * the same in every language, so only the tooltip and the spoken label are translated.
 */
export function PositionalBadge({
  session,
  serverId,
}: {
  session: number;
  serverId?: number | undefined;
}) {
  const t = useT("channels");
  const sending = usePositionalSending(session, serverId);
  if (!sending) return null;
  const tip = t("statusTip.positional");
  return (
    <span className="positional-badge" data-tooltip={tip} role="img" aria-label={tip}>
      <span aria-hidden="true">3D</span>
    </span>
  );
}
