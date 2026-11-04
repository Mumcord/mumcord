import { useTranslation } from "react-i18next";
import type { ShareQuality } from "../../../api";

/** The quality step as the bar and the "Lowered to" note name it: "1080p · 30 fps". */
export function useStepText(): (quality: ShareQuality) => string {
  const { t } = useTranslation("screenshare");
  return (quality) =>
    t("liveBar.qualityBadge", { resolution: `${String(quality.height)}p`, fps: quality.fps });
}
