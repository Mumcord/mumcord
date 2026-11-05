import { useTranslation } from "react-i18next";
import type { PluginRiskChange } from "../../../api";
import { EffectText, ScoreChip } from "../score";
import { scoredOf } from "./reviewData";

/**
 * "Security score: Medium risk · 37 → High risk · 71 ▲ +34 points" (score mockup 04). The engine
 * sends both scores; the difference is only shown, never used to decide anything.
 */
export function ScoreChange({ risk }: { risk: PluginRiskChange }) {
  const { t } = useTranslation("pluginUpdates");
  const delta = risk.toValue - risk.fromValue;
  return (
    <div className="plugin-update__score">
      <span className="plugin-update__score-label">{t("score.label")}</span>
      {delta === 0 ? (
        <>
          <ScoreChip scored={scoredOf(risk.toValue, risk.toBand)} />
          <span className="plugin-update__score-flat">{t("score.unchanged")}</span>
        </>
      ) : (
        <>
          <ScoreChip scored={scoredOf(risk.fromValue, risk.fromBand)} />
          <span aria-hidden="true">→</span>
          <span className="plugin-update__sr-only">{t("versionsTo")}</span>
          <ScoreChip scored={scoredOf(risk.toValue, risk.toBand)} />
          <span className="plugin-update__score-delta" data-dir={delta > 0 ? "up" : "down"}>
            <EffectText effect={{ kind: "points", value: delta }} />
            <span aria-hidden="true"> {t("score.points")}</span>
          </span>
        </>
      )}
    </div>
  );
}
