import { useTranslation } from "react-i18next";
import type { SetupPreset } from "../../api";
import { PresetCards } from "./PresetCards";
import { StepFrame } from "./StepFrame";

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
  return (
    <StepFrame title={t("style.title")} subtitle={t("style.subtitle")} onSubmit={onNext}>
      <PresetCards
        value={style}
        onChange={onSelect}
        legend={t("style.legend")}
        text={(option) => ({
          name: t(`style.${option}.title`),
          tagline: t(`style.${option}.subtitle`),
          points: POINTS[option].map((point) => t(`style.${option}.${point}`)),
        })}
      />
      <p className="wizard-muted" role="status">
        {style === "custom"
          ? t("style.customSelected")
          : t("style.presetSelected", { preset: t(`style.${style}.title`) })}
      </p>
    </StepFrame>
  );
}
