import { useTranslation } from "react-i18next";
import type { NotificationMode, SoundSettings } from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

const EVENTS: (keyof SoundSettings)[] = [
  "joinLeave",
  "voiceJoin",
  "selfMute",
  "selfDeaf",
  "hangUp",
  "quit",
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

/** One switch per notification sound (FR-1.27). */
export function SoundsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("sounds.title")} icon="volume" info={t("sounds.explanation")}>
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
    </Group>
  );
}

/** Which message notifications are shown (FR-1.48c). */
export function NotificationsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("notifications.title")} icon="bell" info={t("notifications.explanation")}>
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
  );
}
