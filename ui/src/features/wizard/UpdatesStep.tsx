import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { StepFrame } from "./StepFrame";

interface UpdatesStepProps {
  /** 1-based step number and step count of this wizard pass (#149 rerun: 3 of 4). */
  step: number;
  total: number;
  initial: boolean;
  focusOnMount: boolean;
  onBack: () => void;
  /** Reports every change, so the wizard already has the answer when Cancel is pressed. */
  onChange?: ((check: boolean) => void) | undefined;
  onNext: () => void;
  /** Rerun only: closes the wizard (#149). */
  onCancel?: (() => void) | undefined;
}

/** Step 4 on first start, step 3 in the rerun: the optional update hint. Off unless the user switches it on (FR-2.56). */
export function UpdatesStep({
  step,
  total,
  initial,
  focusOnMount,
  onBack,
  onChange,
  onNext,
  onCancel,
}: UpdatesStepProps) {
  const { t } = useTranslation("wizard");
  const [check, setCheck] = useState(initial);
  const hintId = useId();
  return (
    <StepFrame
      step={step}
      total={total}
      title={t("updates.title")}
      focusOnMount={focusOnMount}
      busy={false}
      submitLabel={t("common:next")}
      onSubmit={onNext}
      onBack={onBack}
      onCancel={onCancel}
    >
      <div>
        <label className="wizard-radio">
          <input
            type="checkbox"
            checked={check}
            aria-describedby={hintId}
            onChange={(event) => {
              setCheck(event.target.checked);
              onChange?.(event.target.checked);
            }}
          />
          {t("updates.check")}
        </label>
        <p id={hintId} className="wizard-muted wizard-small wizard-indent">
          {t("updates.hint")}
        </p>
      </div>
    </StepFrame>
  );
}
