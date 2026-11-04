import { useTranslation } from "react-i18next";
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
    </Group>
  );
}
