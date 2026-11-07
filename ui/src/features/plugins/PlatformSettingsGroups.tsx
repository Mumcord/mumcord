import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { Group } from "../settings/components/Section";
import { Switch } from "../settings/components/Switch";
import { SliderField } from "../settings/sections/OverlayFields";
import { updatePlatformSettings, usePlatformSettings } from "./platformSettings";

/** "Plugin safety" (FR-9.81, score mockup 05): the switch that blocks high-risk plugins. */
export function PluginSafetyGroup() {
  const { t } = useTranslation("settings");
  const settings = usePlatformSettings();
  return (
    <Group title={t("pluginSafety.title")} icon="shield">
      <Switch
        label={t("pluginSafety.blockHighRisk")}
        checked={settings?.blockHighRisk ?? false}
        disabled={settings === null}
        onChange={(blockHighRisk) => void updatePlatformSettings({ blockHighRisk })}
      />
      <p className="plugins-safety-note">{t("pluginSafety.hint")}</p>
      <p className="plugins-safety-note">{t("pluginSafety.note")}</p>
    </Group>
  );
}

/** "Plugin sounds" (FR-9.68): volume and mute for sounds that plugins play. */
export function PluginSoundsGroup() {
  const { t } = useTranslation("settings");
  const settings = usePlatformSettings();
  return (
    <Group title={t("pluginSounds.title")} icon="volume">
      <SliderField
        label={t("pluginSounds.volume")}
        value={settings?.soundVolume ?? 70}
        min={0}
        max={100}
        step={5}
        unit="%"
        onChange={(soundVolume) => void updatePlatformSettings({ soundVolume })}
      />
      <Switch
        label={t("pluginSounds.mute")}
        checked={settings?.soundMuted ?? false}
        disabled={settings === null}
        onChange={(soundMuted) => void updatePlatformSettings({ soundMuted })}
      />
      <p className="plugins-safety-note">{t("pluginSounds.hint")}</p>
      {/* A debugging aid (#587): only in development builds, not in a release. */}
      {import.meta.env.DEV && <PluginSoundMeter label={t("pluginSounds.meter")} />}
    </Group>
  );
}

/** How often the debug meter reads the plugin-sound output level. */
const METER_INTERVAL_MS = 150;

/** Debug audio meter: the peak of the plugin-sound output, so a played sound is visible. */
export function PluginSoundMeter({ label }: { label: string }) {
  const [level, setLevel] = useState(0);
  useEffect(() => {
    let alive = true;
    const poll = () =>
      void getApi()
        .pluginSoundMeter()
        .then((peak) => {
          if (alive) setLevel(Math.min(1, Math.max(0, peak ?? 0)));
        });
    poll();
    const timer = window.setInterval(poll, METER_INTERVAL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);
  const percent = Math.round(level * 100);
  return (
    <div className="plugins-sound-meter">
      <span>{label}</span>
      <div
        className="plugins-sound-meter-bar"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
      >
        <div className="plugins-sound-meter-fill" style={{ width: `${percent.toString()}%` }} />
      </div>
    </div>
  );
}
