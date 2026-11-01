import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CheckIcon } from "./wizardIcons";
import type { RailEntry } from "./wizardFlow";

interface WizardRailProps {
  entries: readonly RailEntry[];
  /** Share of the path done, 0 – 1. */
  progress: number;
  /** Rerun only: the "identity is not changed" note (#149). */
  note?: ReactNode;
}

/**
 * The step rail (issue #159): every step in order, the current one marked for assistive
 * technology with `aria-current="step"`, done steps with a check, Custom steps tagged, and the
 * progress bar.
 */
export function WizardRail({ entries, progress, note }: WizardRailProps) {
  const { t } = useTranslation("wizard");
  const percent = Math.round(progress * 100);
  return (
    <nav className="wizard-rail" aria-label={t("rail.label")}>
      <div className="wizard-rail__brand">
        <img src={`${import.meta.env.BASE_URL}favicon.png`} alt="" width={40} height={40} />
        <span>
          <span className="wizard-rail__name">{t("rail.appName")}</span>
          <span className="wizard-rail__setup">{t("rail.setup")}</span>
        </span>
      </div>
      <ol className="wizard-rail__steps">
        {entries.map((entry, index) => (
          <li
            key={entry.id}
            className="wizard-rail__step"
            data-state={entry.state}
            aria-current={entry.state === "current" ? "step" : undefined}
          >
            <span className="wizard-rail__marker" aria-hidden="true">
              {entry.state === "done" ? <CheckIcon /> : index + 1}
            </span>
            <span className="wizard-rail__label">{t(`steps.${entry.id}`)}</span>
            {entry.state === "done" && <span className="visually-hidden">{t("rail.done")}</span>}
            {entry.custom && <span className="wizard-rail__tag">{t("rail.custom")}</span>}
          </li>
        ))}
      </ol>
      <div className="wizard-rail__footer">
        <div
          className="wizard-rail__progress"
          role="progressbar"
          aria-label={t("rail.progress")}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <div className="wizard-rail__progress-fill" style={{ width: `${String(percent)}%` }} />
        </div>
        {note}
        <p className="wizard-small wizard-muted">{t("rail.later")}</p>
      </div>
    </nav>
  );
}
