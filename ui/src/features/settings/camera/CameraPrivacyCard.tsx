import { useTranslation } from "react-i18next";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "../sections/types";
import { InfoLineIcon } from "./InfoLineIcon";

/** Privacy card of Settings → Camera (mockup 11, FR-8.21, FR-8.77). */
export function CameraPrivacyCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("camera.privacyTitle")} icon="lock">
      <Switch
        label={t("camera.lockOff")}
        checked={settings.cameraOffOnLock}
        onChange={(cameraOffOnLock) => {
          update({ cameraOffOnLock });
        }}
      />
      <p className="settings-infoline">
        <InfoLineIcon name="info" />
        <span>{t("camera.privacyNote")}</span>
      </p>
    </Group>
  );
}
