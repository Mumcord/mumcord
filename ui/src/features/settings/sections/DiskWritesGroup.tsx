import { useTranslation } from "react-i18next";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/**
 * Spec 12: switches that trade durability and offline copies for fewer disk writes. All default
 * on; each is independent.
 */
export function DiskWritesGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("diskWrites.title")} icon="storage" info={t("diskWrites.hint")}>
      <Switch
        label={t("diskWrites.durableSaves")}
        description={t("diskWrites.durableSavesHint")}
        checked={settings.durableSaves}
        onChange={(durableSaves) => {
          update({ durableSaves });
        }}
      />
      <Switch
        label={t("diskWrites.linkPreviewsOnDisk")}
        description={t("diskWrites.linkPreviewsOnDiskHint")}
        checked={settings.linkPreviewsOnDisk}
        onChange={(linkPreviewsOnDisk) => {
          update({ linkPreviewsOnDisk });
        }}
      />
      <Switch
        label={t("diskWrites.pluginRegistryCache")}
        description={t("diskWrites.pluginRegistryCacheHint")}
        checked={settings.pluginRegistryCache}
        onChange={(pluginRegistryCache) => {
          update({ pluginRegistryCache });
        }}
      />
    </Group>
  );
}
