import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { SetupPreset } from "../../api";
import { RECOMMENDED_PRESET } from "./presets";
import { StepFrame } from "./StepFrame";
import { CheckIcon, StyleIcon } from "./wizardIcons";

/** Card order on the step; Custom comes last. */
const STYLES: readonly SetupPreset[] = ["everythingOn", "balanced", "maximumPrivacy", "custom"];

/** The plain consequences listed on each card (3–4 each). */
const POINTS: Record<SetupPreset, readonly string[]> = {
  everythingOn: ["p1", "p2", "p3", "p4"],
  balanced: ["p1", "p2", "p3", "p4"],
  maximumPrivacy: ["p1", "p2", "p3", "p4"],
  custom: ["p1", "p2", "p3"],
};

interface SetupStyleStepProps {
  style: SetupPreset;
  onSelect: (style: SetupPreset) => void;
  onNext: () => void;
}

/**
 * "How much do you want to share?" (FR-3.116): three presets and Custom as radio cards; arrow
 * keys move between them. A preset jumps to the summary, Custom walks the detailed steps.
 */
export function SetupStyleStep({ style, onSelect, onNext }: SetupStyleStepProps) {
  const { t } = useTranslation("wizard");
  const id = useId();
  return (
    <StepFrame title={t("style.title")} subtitle={t("style.subtitle")} onSubmit={onNext}>
      <fieldset className="wizard-style-cards">
        <legend className="visually-hidden">{t("style.legend")}</legend>
        {STYLES.map((option) => {
          const titleId = `${id}-${option}-title`;
          const subtitleId = `${id}-${option}-subtitle`;
          const listId = `${id}-${option}-list`;
          const selected = option === style;
          return (
            <label key={option} className="wizard-style-card" data-selected={selected}>
              <span className="wizard-style-card__top">
                <span className="wizard-tile">
                  <StyleIcon style={option} />
                </span>
                <input
                  type="radio"
                  className="wizard-style-card__radio"
                  name={id}
                  value={option}
                  checked={selected}
                  aria-labelledby={titleId}
                  aria-describedby={`${subtitleId} ${listId}`}
                  onChange={() => {
                    onSelect(option);
                  }}
                />
                <span className="wizard-style-card__check" aria-hidden="true">
                  {selected && <CheckIcon />}
                </span>
              </span>
              <span className="wizard-style-card__heading">
                <span id={titleId} className="wizard-style-card__name">
                  {option === RECOMMENDED_PRESET && (
                    <>
                      <span className="wizard-badge wizard-badge--recommended">
                        {t("recommended")}
                      </span>{" "}
                    </>
                  )}
                  <span className="wizard-style-card__title">{t(`style.${option}.title`)}</span>
                </span>
                <span id={subtitleId} className="wizard-style-card__subtitle">
                  {t(`style.${option}.subtitle`)}
                </span>
              </span>
              {/* Spans, not a list: a label may only hold phrasing content. */}
              <span id={listId} className="wizard-style-card__points">
                {POINTS[option].map((point) => (
                  <span key={point} className="wizard-style-card__point">
                    {t(`style.${option}.${point}`)}{" "}
                  </span>
                ))}
              </span>
            </label>
          );
        })}
      </fieldset>
      <p className="wizard-muted" role="status">
        {style === "custom"
          ? t("style.customSelected")
          : t("style.presetSelected", { preset: t(`style.${style}.title`) })}
      </p>
    </StepFrame>
  );
}
