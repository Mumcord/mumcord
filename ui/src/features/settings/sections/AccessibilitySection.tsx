import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { applyHintPreferences, type HintPreference } from "../../../a11y";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

const SWITCHES: HintPreference[] = ["keyboardHints", "announcements", "skipLinks"];

/**
 * Switches for the on-screen keyboard hints, screen-reader announcements and skip links
 * (FR-1.69). Saved in the settings record and applied immediately.
 */
export function AccessibilityCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const { keyboardHints, announcements, skipLinks } = settings;

  // Instant effect; a failed save that rolls the record back rolls the switches back too.
  useEffect(() => {
    applyHintPreferences({ keyboardHints, announcements, skipLinks });
  }, [keyboardHints, announcements, skipLinks]);

  return (
    <Group title={t("accessibility.hints")} icon="eye">
      {SWITCHES.map((key) => (
        <Switch
          key={key}
          label={t(`accessibility.${key}`)}
          description={t(`accessibility.${key}Hint`)}
          checked={settings[key]}
          onChange={(value) => {
            update({ [key]: value });
          }}
        />
      ))}
    </Group>
  );
}
