import { useTranslation } from "react-i18next";
import type { PluginBand } from "../../../bindings/commands";
import "./score.css";

/** Where the bands start on the 0–100 scale (FR-9.71); drawn as ticks, not used to compute. */
const BAND_EDGES = [30, 60] as const;

export interface ScoreMeterProps {
  value: number;
  band: PluginBand;
  /** Accessible name; defaults to "Security score". The update prompt names "now" and "after". */
  label?: string;
}

/** A static bar from 0 to 100 with the band edges marked (§5.6). It never animates. */
export function ScoreMeter({ value, band, label }: ScoreMeterProps) {
  const { t } = useTranslation("pluginScore");
  return (
    <div
      className="risk-meter"
      data-level={band}
      role="meter"
      aria-label={label ?? t("meterLabel")}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value}
      aria-valuetext={t("meterValueText", { score: value, band: t(`bandInSentence.${band}`) })}
    >
      <div className="risk-meter__track">
        <span className="risk-meter__fill" style={{ width: `${String(value)}%` }} />
        {BAND_EDGES.map((edge) => (
          <span key={edge} className="risk-meter__tick" style={{ left: `${String(edge)}%` }} />
        ))}
      </div>
      <div className="risk-meter__scale" aria-hidden="true">
        <span>{t("bandEdges.low")}</span>
        <span>{t("bandEdges.medium")}</span>
        <span>{t("bandEdges.high")}</span>
      </div>
    </div>
  );
}
