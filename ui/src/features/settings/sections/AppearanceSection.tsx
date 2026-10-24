import { useTranslation } from "react-i18next";
import { RadioGroup } from "../components/RadioGroup";
import { Group, Section } from "../components/Section";
import type { SettingsSectionProps } from "./types";

/** Appearance: theme override, plus what closing the window does while connected (FR-1.38). */
export function AppearanceSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.appearance")}>
      <Group title={t("appearance.theme")}>
        <RadioGroup
          legend={t("appearance.themeLegend")}
          value={settings.theme}
          options={[
            { value: "system", label: t("appearance.system") },
            { value: "dark", label: t("appearance.dark") },
            { value: "light", label: t("appearance.light") },
          ]}
          onChange={(theme) => {
            update({ theme });
          }}
        />
      </Group>
      <Group title={t("appearance.behaviour")}>
        <RadioGroup
          legend={t("appearance.closeLegend")}
          value={settings.closeBehaviour}
          options={[
            { value: "ask", label: t("appearance.closeAsk") },
            { value: "minimizeToTray", label: t("appearance.closeTray") },
            { value: "quit", label: t("appearance.closeQuit") },
          ]}
          onChange={(closeBehaviour) => {
            update({ closeBehaviour });
          }}
        />
      </Group>
    </Section>
  );
}
