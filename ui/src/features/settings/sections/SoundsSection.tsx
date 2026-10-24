import { useTranslation } from "react-i18next";
import type { SoundSettings } from "../../../api";
import { Section } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

const EVENTS: (keyof SoundSettings)[] = ["joinLeave", "selfMute", "selfDeaf", "messages"];

/** Sounds: one switch per notification sound (FR-1.27). */
export function SoundsSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.sounds")}>
      <p className="settings-hint">{t("sounds.explanation")}</p>
      {EVENTS.map((event) => (
        <Switch
          key={event}
          label={t(`sounds.${event}`)}
          checked={settings.sounds[event]}
          onChange={(enabled) => {
            update({ sounds: { ...settings.sounds, [event]: enabled } });
          }}
        />
      ))}
    </Section>
  );
}
