/**
 * The short notes inside the sharer's live bar: "Lowered to {step}: {reason}" while the share
 * runs below the sharer's pick (FR-7.38), and "Switched to VP9 for {name}" while a viewer who
 * cannot play AV1 watches (FR-7.58).
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { LoweredReason, ShareQuality } from "../../../api";
import { codecName } from "../quality/qualityModel";
import { useStepText } from "../quality/useStepText";
import { useAnnounceChange } from "./useAnnounceChange";
import "./notices.css";

const REASON_KEY: Record<LoweredReason, string> = {
  uploadTooSlow: "qualityMenu.reasonUpload",
  encoderTooSlow: "qualityMenu.reasonEncoder",
  manyViewers: "qualityMenu.reasonViewers",
};

export function BarNotes({
  quality,
  nameOf,
}: {
  quality: ShareQuality | null;
  nameOf: (session: number) => string;
}): ReactNode {
  const { t } = useTranslation("screenshare");
  const stepText = useStepText();
  const lowered =
    quality?.lowered == null || quality.height === 0
      ? null
      : t("qualityMenu.lowered", {
          step: stepText(quality),
          reason: t(REASON_KEY[quality.lowered]),
        });
  const switched = quality?.switched ?? null;
  const forViewer =
    switched?.reason.kind === "viewer"
      ? t("notices.switchedFor", {
          codec: codecName(switched.to.codec),
          name: nameOf(switched.reason.session),
        })
      : null;
  useAnnounceChange(lowered);
  useAnnounceChange(forViewer);
  if (lowered === null && forViewer === null) return null;
  return (
    <span className="share-bar-notes">
      {lowered !== null && <span className="share-bar-note">{lowered}</span>}
      {forViewer !== null && <span className="share-bar-note">{forViewer}</span>}
    </span>
  );
}
