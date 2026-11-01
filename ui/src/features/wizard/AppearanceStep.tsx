import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { Theme } from "../../api";
import { usePreferences } from "../../app/preferences";
import { ThemePicker } from "../../components/ThemePicker";
import { swatchOf } from "../../components/themeSwatch";
import { fromStoredTheme, selectedAppThemeId } from "../../theme/appTheme";
import { PRESET_THEMES } from "../../theme/themeModel";
import { Switch } from "../settings/components/Switch";
import type { SettingsSectionProps } from "../settings/sections/types";
import { StepFrame } from "./StepFrame";

const MODES: readonly Theme[] = ["light", "dark", "system"];

/**
 * "Pick your look" (Custom only, optional): light / dark / system, the theme presets and the
 * visual effects, with the keys of Settings → Appearance. The wizard applies the draft to the
 * window as it changes, so the whole window is the live preview.
 */
export function AppearanceStep({
  settings,
  update,
  onNext,
}: SettingsSectionProps & { onNext: () => void }) {
  const { t } = useTranslation("wizard");
  const { t: ts } = useTranslation("settings");
  const { resolvedTheme } = usePreferences();
  const name = useId();
  const customThemes = useMemo(
    () => settings.customThemes.map(fromStoredTheme),
    [settings.customThemes],
  );
  return (
    <StepFrame title={t("appearance.title")} subtitle={t("appearance.subtitle")} onSubmit={onNext}>
      <fieldset className="wizard-segmented">
        <legend className="wizard-section-title">{ts("appearance.modeLegend")}</legend>
        {MODES.map((mode) => (
          <label
            key={mode}
            className="wizard-segmented__option"
            data-selected={settings.theme === mode}
          >
            <input
              type="radio"
              className="visually-hidden"
              name={name}
              value={mode}
              checked={settings.theme === mode}
              onChange={() => {
                update({ theme: mode });
              }}
            />
            {ts(`appearance.${mode}`)}
          </label>
        ))}
      </fieldset>
      <ThemePicker
        label={ts("appearance.themeLegend")}
        presets={PRESET_THEMES.map((theme) => swatchOf(theme, resolvedTheme))}
        customThemes={customThemes.map((theme) => swatchOf(theme, resolvedTheme))}
        selectedId={selectedAppThemeId(settings.appThemeId, customThemes)}
        onSelect={(appThemeId) => {
          update({ appThemeId });
        }}
      />
      <section className="wizard-card" aria-label={ts("appearanceEffects.title")}>
        <Switch
          label={ts("appearanceEffects.visualEffects")}
          tip={{
            label: t("tipLabel", { name: ts("appearanceEffects.visualEffects") }),
            text: ts("appearanceEffects.visualEffectsHint"),
          }}
          checked={settings.visualEffects}
          onChange={(visualEffects) => {
            update({ visualEffects });
          }}
        />
      </section>
    </StepFrame>
  );
}
