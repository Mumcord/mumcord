import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { UPDATE_MODES, type UpdateMode } from "../updates/updateMode";
import { StepFrame } from "./StepFrame";

interface UpdatesStepProps {
  /** 1-based step number and step count of this wizard pass (#149 rerun: 3 of 4). */
  step: number;
  total: number;
  initial: UpdateMode;
  focusOnMount: boolean;
  onBack: () => void;
  /** Reports every change, so the wizard already has the answer when Cancel is pressed. */
  onChange?: ((mode: UpdateMode) => void) | undefined;
  onNext: () => void;
  /** Rerun only: closes the wizard (#149). */
  onCancel?: (() => void) | undefined;
}

const LABELS = {
  auto: { label: "updates.auto", hint: "updates.autoHint" },
  notify: { label: "updates.notify", hint: "updates.notifyHint" },
  off: { label: "updates.off", hint: "updates.offHint" },
} as const;

/**
 * Step 4 on first start, step 3 in the rerun: what Mumcord does about new versions. Off unless
 * the user picks something else: nothing is sent to GitHub before that (FR-2.56, FR-3.146).
 */
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
  const [mode, setMode] = useState(initial);
  const baseId = useId();
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
      <fieldset className="wizard-choice">
        <legend className="visually-hidden">{t("updates.choice")}</legend>
        {UPDATE_MODES.map((option) => {
          const hintId = `${baseId}-${option}`;
          return (
            <div key={option}>
              <label className="wizard-radio">
                <input
                  type="radio"
                  name={baseId}
                  value={option}
                  checked={mode === option}
                  aria-describedby={hintId}
                  onChange={() => {
                    setMode(option);
                    onChange?.(option);
                  }}
                />
                {t(LABELS[option].label)}
              </label>
              <p id={hintId} className="wizard-muted wizard-small wizard-indent">
                {t(LABELS[option].hint)}
              </p>
            </div>
          );
        })}
      </fieldset>
      <p className="wizard-muted wizard-small">{t("updates.privacy")}</p>
    </StepFrame>
  );
}
