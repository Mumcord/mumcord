import { useId } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type NotificationMode, type SoundSettings, type SoundTheme } from "../../../api";
import { RadioGroup } from "../components/RadioGroup";
import { Group } from "../components/Section";
import { ChangedBadge } from "../components/SettingBadges";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";
import "./updates.css";

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
const SOUND_THEMES: SoundTheme[] = ["rich", "classic"];
const THEME_LABELS: Record<SoundTheme, string> = {
  classic: "sounds.themeClassic",
  rich: "sounds.themeRich",
};

/** Sound theme (#325) and one switch per notification sound (FR-1.27). */
export function SoundsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("sounds.title")} icon="volume" info={t("sounds.explanation")}>
      <SoundThemeRow
        theme={settings.soundTheme}
        onChange={(soundTheme) => {
          update({ soundTheme });
        }}
      />
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

/** Classic or Rich sounds as a segmented control, with a preview of the chosen theme. */
function SoundThemeRow(props: { theme: SoundTheme; onChange: (theme: SoundTheme) => void }) {
  const { t } = useTranslation("settings");
  const titleId = useId();
  const hintId = useId();
  const label = t("sounds.theme");
  return (
    <div className="upd-settings-row">
      <div className="upd-settings-row__text">
        <p className="upd-settings-row__title">
          <span id={titleId}>{label}</span>
          <span className="setting-badges">
            <ChangedBadge label={label} />
          </span>
        </p>
        <p id={hintId} className="settings-hint">
          {t("sounds.previewHint")}
        </p>
      </div>
      <div role="radiogroup" className="upd-segmented" aria-labelledby={titleId}>
        {SOUND_THEMES.map((theme) => (
          <label
            key={theme}
            className={`upd-segmented__option${props.theme === theme ? " upd-segmented__option--on" : ""}`}
          >
            <input
              type="radio"
              className="visually-hidden"
              name={titleId}
              value={theme}
              checked={props.theme === theme}
              onChange={() => {
                props.onChange(theme);
              }}
            />
            {t(THEME_LABELS[theme])}
          </label>
        ))}
      </div>
      <button
        type="button"
        className="settings-button"
        aria-describedby={hintId}
        onClick={() => {
          getApi()
            .audioPreviewSoundTheme(props.theme)
            .catch((error: unknown) => {
              console.error("Previewing the sound theme failed", error);
            });
        }}
      >
        {t("sounds.preview")}
      </button>
    </div>
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
