import { useTranslation } from "react-i18next";

export type StepState = "done" | "current" | "pending";

export interface Step {
  label: string;
  state: StepState;
}

/** The steps of an update with a mark for each; the state is spelled out for screen readers. */
export function StepList({ steps }: { steps: readonly Step[] }) {
  const { t } = useTranslation("appwindow");
  return (
    <ol className="upd-steps">
      {steps.map((step) => (
        <li
          key={step.label}
          className={`upd-step upd-step--${step.state}`}
          aria-current={step.state === "current" ? "step" : undefined}
        >
          <span className="upd-step__mark" aria-hidden="true">
            {step.state === "done" ? "✓" : ""}
          </span>
          {step.label} <span className="visually-hidden">{t(`updater.steps.${step.state}`)}</span>
        </li>
      ))}
    </ol>
  );
}
