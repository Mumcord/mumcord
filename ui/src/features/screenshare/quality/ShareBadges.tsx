/**
 * The badges in the sharer's live bar (FR-7.39, mockups 02, 06, 07): the quality step the share
 * runs at, the encoder it runs on and how it reaches the viewers.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ShareQuality } from "../../../api";
import { BoltIcon, RelayIcon } from "../icons";
import { codecBadge, sharePath } from "./qualityModel";
import { useStepText } from "./useStepText";
import "./quality.css";

export function ShareBadges({ quality }: { quality: ShareQuality }): ReactNode {
  const { t } = useTranslation("screenshare");
  const stepText = useStepText();
  const codec = codecBadge(quality.encoder);
  const path = sharePath(quality.viewers);
  return (
    <span className="share-badges">
      {quality.height > 0 && (
        <span className="share-badge" data-tone={quality.lowered === null ? undefined : "warn"}>
          {stepText(quality)}
        </span>
      )}
      {codec !== null && (
        <span
          className="share-badge"
          data-tone={
            quality.switched !== null ? "warn" : codec.codec === "AV1" ? "accent" : undefined
          }
        >
          {t(codec.hardware ? "liveBar.codecBadgeHardware" : "liveBar.codecBadgeCpu", {
            codec: codec.codec,
          })}
        </span>
      )}
      {path !== null && (
        <span className="share-badge" data-tone={path === "relay" ? "ok" : undefined}>
          {path === "relay" ? <RelayIcon /> : <BoltIcon />}
          {t(path === "relay" ? "liveBar.pathRelay" : "liveBar.pathDirect")}
        </span>
      )}
    </span>
  );
}
