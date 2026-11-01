import { useContext, useEffect, useId, useRef, type ReactNode, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { StepNavContext, type StepNav } from "./stepNav";

function useStepNav(): StepNav {
  const nav = useContext(StepNavContext);
  if (nav === null) throw new Error("StepFrame needs a StepNavContext");
  return nav;
}

const PRIMARY_LABELS = {
  next: "common:next",
  getStarted: "nav.getStarted",
  finishSetup: "nav.finishSetup",
  startMumcord: "nav.startMumcord",
  finish: "nav.finish",
} as const;

/** Radios and switches do not submit a form on Enter by themselves; in the wizard they do. */
const ENTER_SUBMITS = new Set(["radio", "checkbox"]);

interface StepFrameProps {
  title: ReactNode;
  /** Plain text of the title for the announcement, when the title is not a string. */
  titleText?: string;
  subtitle?: string;
  /** Saving or validating: the primary button does nothing until it is done. */
  busy?: boolean;
  onSubmit: () => void;
  /** Extra footer content next to the buttons, e.g. "Next: add your first server". */
  footerNote?: ReactNode;
  /** Shown before the heading, e.g. the summary's check mark. */
  badge?: ReactNode;
  children: ReactNode;
}

/** One wizard step: "STEP n OF m · NAME", heading, subtitle, content and the footer buttons. */
export function StepFrame({
  title,
  titleText,
  subtitle,
  busy = false,
  onSubmit,
  footerNote,
  badge,
  children,
}: StepFrameProps) {
  const { t } = useTranslation("wizard");
  const nav = useStepNav();
  const headingId = useId();
  const progressId = useId();
  const heading = useRef<HTMLHeadingElement>(null);
  const form = useRef<HTMLFormElement>(null);
  const progress = t("progress", { current: nav.number, total: nav.total });
  const spokenTitle = titleText ?? (typeof title === "string" ? title : "");

  // On a step change, focus the new heading and announce it politely for screen readers that
  // do not read a focused heading's description.
  useEffect(() => {
    if (!nav.focusOnMount) return;
    heading.current?.focus();
    announce(`${spokenTitle}. ${progress}`, "polite");
    // Only when the step appears, not on re-renders.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function submit(event: SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!busy) onSubmit();
  }

  // Enter on a radio or switch continues, as on a text field. A native listener, since the
  // form itself is no interactive element.
  useEffect(() => {
    const element = form.current;
    if (element === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        event.key === "Enter" &&
        !event.defaultPrevented &&
        target instanceof HTMLInputElement &&
        ENTER_SUBMITS.has(target.type)
      ) {
        event.preventDefault();
        element.requestSubmit();
      }
    };
    element.addEventListener("keydown", onKeyDown);
    return () => {
      element.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  return (
    <form
      ref={form}
      className="wizard-step"
      aria-labelledby={headingId}
      onSubmit={submit}
      noValidate
    >
      <div className="wizard-step__body">
        <header className="wizard-step__header">
          {badge}
          <div>
            <p id={progressId} className="wizard-eyebrow">
              {progress} · {nav.name}
            </p>
            <h2
              id={headingId}
              ref={heading}
              className="wizard-step__title"
              tabIndex={-1}
              aria-describedby={progressId}
            >
              {title}
            </h2>
            {subtitle !== undefined && <p className="wizard-step__subtitle">{subtitle}</p>}
          </div>
        </header>
        {children}
      </div>
      <div className="wizard-footer">
        <p className="wizard-footer__hint" aria-hidden="true">
          <kbd className="wizard-kbd">{t("nav.enterKey")}</kbd> {t("nav.enterHint")}
        </p>
        {nav.onCancel && (
          <button type="button" className="button" onClick={nav.onCancel}>
            {t("common:cancel")}
          </button>
        )}
        {nav.onSkip && (
          <button type="button" className="wizard-footer__skip" onClick={nav.onSkip}>
            {t("nav.skip")}
          </button>
        )}
        {footerNote}
        {nav.onBack && (
          <button type="button" className="button" onClick={nav.onBack}>
            <span aria-hidden="true">← </span>
            {t("common:back")}
          </button>
        )}
        <button
          type="submit"
          className="button button--primary"
          aria-disabled={busy}
          aria-busy={busy}
        >
          {t(PRIMARY_LABELS[nav.primary])}
          <span aria-hidden="true"> →</span>
        </button>
      </div>
    </form>
  );
}
