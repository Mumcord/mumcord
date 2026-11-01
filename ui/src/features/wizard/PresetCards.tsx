import { useId } from "react";
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

/** What one card says; the caller supplies it, as Custom means something else in each place. */
export interface PresetCardText {
  name: string;
  tagline: string;
  /** The plain consequences, 3–4 per card. */
  points: readonly string[];
}

/**
 * The setup style cards of #159 (Everything on / Balanced, Recommended / Maximum privacy /
 * Custom), each with its plain consequences, shared by the wizard's Setup style step and
 * Settings → Switch preset. Native radios, so arrow keys move between them; the name names each
 * radio, the tagline and points describe it.
 */
export function PresetCards({
  value,
  onChange,
  legend,
  text,
}: {
  value: SetupPreset | null;
  onChange: (preset: SetupPreset) => void;
  /** Accessible name of the group. */
  legend: string;
  text: (preset: SetupPreset) => PresetCardText;
}) {
  const id = useId();
  return (
    <fieldset className="preset-cards">
      <legend className="visually-hidden">{legend}</legend>
      {SETUP_STYLES.map((preset) => {
        const titleId = `${id}-${preset}-title`;
        const describedBy = `${id}-${preset}-tagline ${id}-${preset}-points`;
        const recommended = preset === RECOMMENDED_PRESET;
        const { name, tagline, points } = text(preset);
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
              <span className="preset-card__name">{name}</span>
              {recommended && (
                <>
                  {" "}
                  <RecommendedBadge />
                </>
              )}
            </span>
            <span id={`${id}-${preset}-tagline`} className="preset-card__tagline">
              {tagline}
            </span>
            {/* Spans, not a list: a label may only hold phrasing content. */}
            <span id={`${id}-${preset}-points`} className="preset-card__points">
              {points.map((point) => (
                <span key={point} className="preset-card__point">
                  {point}{" "}
                </span>
              ))}
            </span>
          </label>
        );
      })}
    </fieldset>
  );
}
