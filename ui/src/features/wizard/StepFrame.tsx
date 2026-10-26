import { useEffect, useId, useRef, type SubmitEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";

export const STEP_COUNT = 5;

interface StepFrameProps {
  /** 1-based step number. */
  step: number;
  title: string;
  /** Move focus to the heading when the step appears (not on the very first render). */
  focusOnMount: boolean;
  busy: boolean;
  submitLabel: string;
  onSubmit: () => void;
  onBack?: () => void;
  children: ReactNode;
}

/** Shared layout of one wizard step: progress, heading, content, Back/Next. */
export function StepFrame({
  step,
  title,
  focusOnMount,
  busy,
  submitLabel,
  onSubmit,
  onBack,
  children,
}: StepFrameProps) {
  const { t } = useTranslation("wizard");
  const { t: tc } = useTranslation("common");
  const headingId = useId();
  const progressId = useId();
  const heading = useRef<HTMLHeadingElement>(null);

  const progress = t("progress", { current: step, total: STEP_COUNT });

  // On a step change, focus the new heading and announce it politely for screen readers that
  // do not read a focused heading's description.
  useEffect(() => {
    if (!focusOnMount) return;
    heading.current?.focus();
    announce(`${title}. ${progress}`, "polite");
    // Only when the step appears, not on re-renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function submit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!busy) onSubmit();
  }

  return (
    <form className="wizard-step" aria-labelledby={headingId} onSubmit={submit} noValidate>
      <p id={progressId} className="wizard-progress">
        {progress}
      </p>
      <h2 id={headingId} ref={heading} tabIndex={-1} aria-describedby={progressId}>
        {title}
      </h2>
      {children}
      <div className="wizard-nav">
        {onBack && (
          <button type="button" className="button" onClick={onBack}>
            {tc("back")}
          </button>
        )}
        <button
          type="submit"
          className="button button--primary"
          aria-disabled={busy}
          aria-busy={busy}
        >
          {submitLabel}
        </button>
      </div>
    </form>
  );
}
