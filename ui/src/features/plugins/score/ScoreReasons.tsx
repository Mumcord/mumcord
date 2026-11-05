import { useTranslation } from "react-i18next";
import type { PluginScoreFactor } from "../../../bindings/commands";
import { biggestReasons, breakdownRows } from "./breakdownRows";
import { useFactorName } from "./useFactorName";
import "./score.css";

/** "Biggest reasons" of the install prompt (FR-9.77, mockup 03), from the engine's factors. */
export function ScoreReasons({ factors }: { factors: readonly PluginScoreFactor[] }) {
  const { t } = useTranslation("pluginScore");
  const factorName = useFactorName();
  const reasons = biggestReasons(breakdownRows(factors));
  if (reasons.length === 0) return null;
  return (
    <ul className="risk-reasons" aria-label={t("biggestReasons")}>
      {reasons.map((row, i) => {
        const name = factorName(row.name);
        const percent = row.effect.kind === "percent";
        return (
          <li key={i}>
            <span>{percent ? t("reasonWorse", { factor: name }) : name}</span>
            <strong>
              {percent
                ? t("effect.percentLabel", { sign: "+", count: row.effect.value })
                : t("effect.pointsLabel", { count: row.effect.value })}
            </strong>
          </li>
        );
      })}
    </ul>
  );
}
