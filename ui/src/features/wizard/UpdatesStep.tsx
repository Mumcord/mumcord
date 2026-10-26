import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { StepFrame } from "./StepFrame";

interface UpdatesStepProps {
  initial: boolean;
  focusOnMount: boolean;
  onBack: () => void;
  onNext: (check: boolean) => void;
}

/** Step 4: the optional update hint. Off unless the user switches it on (FR-2.56). */
export function UpdatesStep({ initial, focusOnMount, onBack, onNext }: UpdatesStepProps) {
  const { t } = useTranslation("wizard");
  const [check, setCheck] = useState(initial);
  const hintId = useId();
  return (
    <StepFrame
      step={4}
      title={t("updates.title")}
      focusOnMount={focusOnMount}
      busy={false}
      submitLabel={t("common:next")}
      onSubmit={() => {
        onNext(check);
      }}
      onBack={onBack}
    >
      <div>
        <label className="wizard-radio">
          <input
            type="checkbox"
            checked={check}
            aria-describedby={hintId}
            onChange={(event) => {
              setCheck(event.target.checked);
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
