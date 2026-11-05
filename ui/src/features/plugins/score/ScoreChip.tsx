import { useTranslation } from "react-i18next";
import type { PluginBand, PluginScored } from "../../../bindings/commands";
import { ShieldIcon, type ShieldKind } from "./ShieldIcon";
import "./score.css";

const BAND_SHIELD: Record<PluginBand, ShieldKind> = {
  low: "check",
  medium: "exclamation",
  high: "cross",
};

export interface ScoreChipProps {
  /** The engine's result as `pluginScore` returns it; the UI never computes a score. */
  scored: PluginScored;
  /** "Don't allow high-risk plugins" is on and this plugin is high risk (mockup 07). */
  blocked?: boolean;
  /** A plugin loaded in Developer mode: scored for information only (mockup 07). */
  developer?: boolean;
  /** Icon size: 14 px in lists and prompts, 16 px on the details score card. */
  size?: "sm" | "md";
}

/**
 * The security score label (FR-9.71, §5.6): colour, icon, word and number together, text first.
 * The visible words are hidden from screen readers in favour of one complete sentence.
 */
export function ScoreChip({
  scored,
  blocked = false,
  developer = false,
  size = "sm",
}: ScoreChipProps) {
  const { t } = useTranslation("pluginScore");
  const iconSize = size === "md" ? 16 : 14;

  if (scored.kind === "official") {
    return (
      <span className="risk-chip" data-level="official">
        <ShieldIcon kind="check" size={iconSize} />
        <span aria-hidden="true">{t("official")}</span>
        <span className="visually-hidden">{t("officialLabel")}</span>
      </span>
    );
  }
  if (scored.kind === "refused" || blocked) {
    const refused = scored.kind === "refused";
    return (
      <span className="risk-chip" data-level="blocked">
        <ShieldIcon kind="blocked" size={iconSize} />
        {refused ? (
          <>
            <span aria-hidden="true">{t("legend.cannotInstall")}</span>
            <span className="visually-hidden">{t("hardStop.title")}</span>
          </>
        ) : (
          <span>{t("blocked.setting")}</span>
        )}
      </span>
    );
  }

  const { value, band } = scored.score;
  const bandText = t(`band.${band}`);
  const chip = (
    <span className="risk-chip" data-level={band}>
      <ShieldIcon kind={BAND_SHIELD[band]} size={iconSize} />
      <span aria-hidden="true">{bandText}</span>
      <span className="risk-chip__score" aria-hidden="true">
        · {value}
      </span>
      {!developer && (
        <span className="visually-hidden">{t("chipLabel", { score: value, band: bandText })}</span>
      )}
    </span>
  );
  if (!developer) return chip;
  return (
    <span className="risk-chip-group">
      {chip}
      <span className="risk-chip-note" aria-hidden="true">
        {t("developerPlugin")}
      </span>
      <span className="visually-hidden">
        {t("developerLabel", { score: value, band: bandText })}
      </span>
    </span>
  );
}
