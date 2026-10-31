import { useTranslation } from "react-i18next";
import type { NotificationMode, SoundSettings } from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { DndGroup } from "./DndGroup";
import type { SettingsSectionProps } from "./types";

const EVENTS: (keyof SoundSettings)[] = [
  "joinLeave",
  "voiceJoin",
  "selfMute",
  "selfDeaf",
  "hangUp",
  "kicked",
  "banned",
  "messages",
  "mention",
  "mentionNotify",
  "partyStarted",
  "partyJoined",
  "partyLeft",
  "shareStarted",
  "shareStopped",
  "viewerStarted",
  "viewerStopped",
];
const NOTIFICATION_MODES: NotificationMode[] = ["both", "inApp", "desktop", "off"];

/**
 * Sounds & notifications: Do Not Disturb (FR-3.111), one switch per notification sound
 * (FR-1.27) and which message notifications are shown (FR-1.48c).
 */
export function SoundsSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.sounds")}>
      <DndGroup />
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
      <Group title={t("notifications.title")}>
        <p className="settings-hint">{t("notifications.explanation")}</p>
        <RadioGroup
          legend={t("notifications.legend")}
          value={settings.notifications}
          options={NOTIFICATION_MODES.map((mode) => ({
            value: mode,
            label: t(`notifications.${mode}`),
          }))}
          onChange={(notifications) => {
            update({ notifications });
          }}
        />
      </Group>
    </Section>
  );
}
