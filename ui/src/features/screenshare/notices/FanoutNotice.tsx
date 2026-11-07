/**
 * "This server has no relay for large shares" (FR-7.47, FR-7.49, mockup 06): the card under the
 * live bar when the share sends one copy per viewer instead of once through the relay. It shows
 * once per share and goes away with "Got it".
 */
import { useId, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { FanoutReason, ScreenShare, ShareQuality } from "../../../api";
import { shareStartedAt } from "../sharesStore";
import { dismissFanoutNotice, wasDismissed } from "./fanoutDismissed";
import { MAX_VIEWERS } from "../Viewers";
import { WarningIcon } from "../quality/qualityIcons";
import { useStepText } from "../quality/useStepText";
import { useAnnounceChange } from "./useAnnounceChange";
import { RefusalAction } from "../../p2p/RefusalAction";
import "./notices.css";

const REASON_KEY: Record<FanoutReason, string> = {
  noRelay: "notices.reasonNoRelay",
  oldRelay: "notices.reasonOldRelay",
  disallowed: "notices.reasonDisallowed",
  refused: "notices.reasonRefused",
  full: "notices.reasonFull",
  lost: "notices.reasonLost",
  mode: "notices.reasonMode",
  noCertificate: "notices.reasonNoCertificate",
  unavailable: "notices.reasonUnavailable",
  notRegistered: "notices.reasonNotRegistered",
  groupNotAllowed: "notices.reasonGroupNotAllowed",
  noHello: "notices.reasonNoHello",
};

export function FanoutNotice({
  serverId,
  own,
  quality,
}: {
  serverId: number;
  own: ScreenShare;
  quality: ShareQuality | null;
}): ReactNode {
  const { t } = useTranslation("screenshare");
  const stepText = useStepText();
  const titleId = useId();
  const startedAt = shareStartedAt(own);
  const [hidden, setHidden] = useState(() => wasDismissed(serverId, startedAt));
  const fanout = quality?.fanout ?? null;
  const reason = fanout?.kind === "direct" && !hidden ? fanout.reason : null;
  const unavailable =
    reason === null
      ? null
      : t("notices.fanoutUnavailable", { reason: t(REASON_KEY[reason]), max: MAX_VIEWERS });
  const title = reason === "noRelay" ? t("notices.noFanoutTitle") : unavailable;
  useAnnounceChange(title);
  if (quality === null || reason === null || title === null) return null;

  // Mockup 06 explains the cost once viewers lowered the quality below a pick it can name.
  const count = quality.viewers.length;
  const wanted = quality.choice;
  const detail =
    quality.lowered === "manyViewers" && count > 0 && wanted.resolution !== "auto"
      ? t("notices.noFanoutBody", {
          max: MAX_VIEWERS,
          count,
          mbit: Math.max(1, Math.round((quality.kbps * count) / 1000)),
          now: stepText(quality),
          wanted: t("liveBar.qualityBadge", {
            resolution: `${wanted.resolution}p`,
            fps: Number(wanted.fps),
          }),
        })
      : null;
  const body = detail ?? (title === unavailable ? null : unavailable);

  return (
    <section className="share-notice" aria-labelledby={titleId}>
      <span className="share-notice__icon">
        <WarningIcon />
      </span>
      <div className="share-notice__text">
        <p id={titleId} className="share-notice__title">
          {title}
        </p>
        {body !== null && <p>{body}</p>}
      </div>
      <span className="share-notice__actions">
        {(reason === "notRegistered" || reason === "noHello") && (
          <RefusalAction
            hint={reason === "notRegistered" ? "register" : "reconnect"}
            serverId={serverId}
          />
        )}
        <button
          type="button"
          className="button"
          onClick={() => {
            dismissFanoutNotice(serverId, startedAt);
            setHidden(true);
          }}
        >
          {t("notices.gotIt")}
        </button>
      </span>
    </section>
  );
}
