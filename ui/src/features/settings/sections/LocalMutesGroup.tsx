import { useTranslation } from "react-i18next";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/**
 * "Forget local mutes when Mumcord starts" (FR-15.1, FR-15.5): the clearing runs once at the next
 * app start, so switching it on never unmutes anyone right away.
 */
export function LocalMutesGroup({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("localMutes.title")} icon="volume">
      <Switch
        label={t("localMutes.switch")}
        status={t("localMutes.hint")}
        checked={settings.forgetLocalMutesOnStart}
        onChange={(forgetLocalMutesOnStart) => {
          update({ forgetLocalMutesOnStart });
        }}
      />
    </Group>
  );
}
