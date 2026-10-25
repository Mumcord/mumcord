import { useTranslation } from "react-i18next";
import { setHintPreference, useHintPreferences, type HintPreference } from "../../../a11y";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";

const SWITCHES: HintPreference[] = ["keyboardHints", "announcements", "skipLinks"];

/**
 * Accessibility: switches for the on-screen keyboard hints, screen-reader announcements and skip
 * links. Saved in this window's storage, not the settings record, and applied immediately.
 */
export function AccessibilitySection() {
  const { t } = useTranslation("settings");
  const preferences = useHintPreferences();
  return (
    <Section title={t("sections.accessibility")}>
      <Group title={t("accessibility.hints")}>
        {SWITCHES.map((key) => (
          <Switch
            key={key}
            label={t(`accessibility.${key}`)}
            description={t(`accessibility.${key}Hint`)}
            checked={preferences[key]}
            onChange={(value) => {
              setHintPreference(key, value);
            }}
          />
        ))}
      </Group>
    </Section>
  );
}
