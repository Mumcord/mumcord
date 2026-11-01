import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { SetupPreset } from "../../api";
import { RecommendedBadge } from "../settings/components/SettingBadges";
import { SettingsIcon, type SettingsIconName } from "../settings/components/SettingsIcon";
import { RECOMMENDED_PRESET, SETUP_STYLES } from "./presets";
import "./presetCards.css";

const ICONS: Record<SetupPreset, SettingsIconName> = {
  everythingOn: "star",
  balanced: "scale",
  maximumPrivacy: "shield",
  custom: "sliders",
};

/** How many plain consequences each card lists. */
const POINTS: Record<SetupPreset, number> = {
  everythingOn: 4,
  balanced: 4,
  maximumPrivacy: 4,
  custom: 3,
};

/**
 * The setup style cards of #159 (Everything on / Balanced, Recommended / Maximum privacy /
 * Custom), each with its plain consequences. Native radios, so arrow keys move between them;
 * the name names each radio, the tagline and points describe it.
 */
export function PresetCards({
  value,
  onChange,
  legend,
}: {
  value: SetupPreset | null;
  onChange: (preset: SetupPreset) => void;
  /** Accessible name of the group. */
  legend: string;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <fieldset className="preset-cards">
      <legend className="visually-hidden">{legend}</legend>
      {SETUP_STYLES.map((preset) => {
        const titleId = `${id}-${preset}-title`;
        const describedBy = `${id}-${preset}-tagline ${id}-${preset}-points`;
        const recommended = preset === RECOMMENDED_PRESET;
        return (
          <label key={preset} className="preset-card" data-preset={preset}>
            <span className="preset-card__head">
              <span className="preset-card__icon">
                <SettingsIcon name={ICONS[preset]} />
              </span>
              <input
                type="radio"
                name={id}
                value={preset}
                className="preset-card__radio"
                checked={preset === value}
                aria-labelledby={titleId}
                aria-describedby={describedBy}
                onChange={() => {
                  onChange(preset);
                }}
              />
            </span>
            <span id={titleId} className="preset-card__title">
              {/* Name first, so screen readers say "Balanced, Recommended"; CSS puts the pill on top. */}
              <span className="preset-card__name">{t(`presets.${preset}.name`)}</span>
              {recommended && <RecommendedBadge />}
            </span>
            <span id={`${id}-${preset}-tagline`} className="preset-card__tagline">
              {t(`presets.${preset}.tagline`)}
            </span>
            <ul id={`${id}-${preset}-points`} className="preset-card__points">
              {Array.from({ length: POINTS[preset] }, (_, index) => (
                <li key={index}>{t(`presets.${preset}.point${String(index + 1)}`)}</li>
              ))}
            </ul>
          </label>
        );
      })}
    </fieldset>
  );
}
