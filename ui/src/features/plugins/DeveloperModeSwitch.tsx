import { useTranslation } from "react-i18next";
import { Group } from "../settings/components/Section";
import { Switch } from "../settings/components/Switch";
import type { SettingsSectionProps } from "../settings/sections/types";

/** Settings → Advanced: Developer mode adds the Developer tab to Plugins (FR-6.73). */
export function DeveloperModeCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("developerMode.title")} icon="code">
      <Switch
        label={t("developerMode.switch")}
        description={t("developerMode.hint")}
        checked={settings.developerMode}
        onChange={(developerMode) => {
          update({ developerMode });
        }}
      />
    </Group>
  );
}
