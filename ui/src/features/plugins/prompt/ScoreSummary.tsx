import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PluginInstallScore } from "../../../api";
import { ScoreBreakdown, ScoreChip, ScoreMeter, ScoreReasons } from "../score";

export interface ScoreSummaryProps {
  score: PluginInstallScore;
  pluginName: string;
  /** "Don't allow high-risk plugins" is on and this plugin is High (FR-9.81). */
  blocked: boolean;
  /** Number of websites the package declares, shown in the breakdown. */
  websites: number;
}

/**
 * The score card at the top of the install prompt (score mockup 03): chip, meter, the biggest
 * reasons and the full breakdown on request. It shows the verified package's score, never one the
 * UI worked out (FR-9.76). Official plugins have no score, so nothing is shown for them.
 */
export function ScoreSummary({ score, pluginName, blocked, websites }: ScoreSummaryProps) {
  const { t } = useTranslation("pluginScore");
  const [open, setOpen] = useState(false);
  const breakdownId = useId();
  const scored = score.score;
  if (scored.kind !== "scored") return null;
  const { value, band, factors } = scored.score;
  return (
    <section className="plugin-prompt-score" aria-label={t("title")}>
      <div className="plugin-prompt-score-row">
        <ScoreChip scored={scored} blocked={blocked} size="md" />
        <span className="plugin-prompt-muted">{t("inline", { score: value })}</span>
      </div>
      {score.higherThanListing && (
        <p className="plugin-prompt-score-note" role="note">
          {t("higherThanListing")}
        </p>
      )}
      {blocked && <p className="plugin-prompt-score-note">{t("blocked.settingReason")}</p>}
      <ScoreMeter value={value} band={band} />
      <ScoreReasons factors={factors} />
      <button
        type="button"
        className="plugin-prompt-more"
        aria-expanded={open}
        aria-controls={breakdownId}
        onClick={() => {
          setOpen((current) => !current);
        }}
      >
        {open ? t("hideBreakdown") : t("showBreakdown")}
      </button>
      <div id={breakdownId} hidden={!open}>
        {open && (
          <ScoreBreakdown score={scored.score} pluginName={pluginName} facts={{ websites }} />
        )}
      </div>
    </section>
  );
}
