import { useId } from "react";
import { useTranslation } from "react-i18next";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/** Range of the lowered level, in percent (FR-2.76). */
export const DUCKING_MIN_PERCENT = 10;
export const DUCKING_MAX_PERCENT = 80;

/**
 * Auto-ducking (FR-2.76): lower the watch-party video and watched screen-share sound while people
 * in the voice channel talk, and how far.
 */
export function DuckingGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const id = useId();
  const shown = t("audio.duckingLevelValue", { percent: settings.videoDuckingPercent });
  return (
    <Group title={t("audio.videoSound")}>
      <Switch
        label={t("audio.ducking")}
        description={t("audio.duckingHint")}
        checked={settings.videoDucking}
        onChange={(videoDucking) => {
          update({ videoDucking });
        }}
      />
      <div className="settings-field overlay-settings__slider">
        <label htmlFor={id}>{t("audio.duckingLevel")}</label>
        <input
          id={id}
          type="range"
          min={DUCKING_MIN_PERCENT}
          max={DUCKING_MAX_PERCENT}
          step={5}
          value={settings.videoDuckingPercent}
          disabled={!settings.videoDucking}
          aria-valuetext={shown}
          onChange={(event) => {
            update({ videoDuckingPercent: Number(event.target.value) });
          }}
        />
        <output htmlFor={id}>{shown}</output>
      </div>
    </Group>
  );
}
