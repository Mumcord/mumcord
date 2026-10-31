import { useTranslation } from "react-i18next";

/**
 * The rerun's step list (#149): the first-start steps with the identity step marked as skipped,
 * because a rerun never touches the identity. Announced as a list, the current step marked.
 */
export function WizardSteps({ current, total }: { current: number; total: number }) {
  const { t } = useTranslation("wizard");
  return (
    <ol className="wizard-steps" aria-label={t("rerun.stepsLabel")}>
      <li className="wizard-steps__item wizard-steps__item--skipped">
        <span className="wizard-steps__chip">{t("rerun.steps.identity")}</span>
        <span className="visually-hidden">{t("rerun.stepsSkipped")}</span>
      </li>
      {["mic", "privacy", "updates", "talk"].map((id, index) => (
        <li
          key={id}
          className="wizard-steps__item"
          aria-current={index + 1 === current ? "step" : undefined}
        >
          <span className="wizard-steps__chip">{t(`rerun.steps.${id}`)}</span>
          <span className="visually-hidden">{t("progress", { current: index + 1, total })}</span>
        </li>
      ))}
    </ol>
  );
}
