import { useTranslation } from "react-i18next";
import { RadioGroup } from "../components/RadioGroup";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { ThemeEditor } from "../theme/ThemeEditor";
import { useAppInfo } from "../useAppInfo";
import { AppImageMenuEntrySwitch } from "./AppImageMenuEntrySwitch";
import type { SettingsSectionProps } from "./types";

/** Colour theme and its editor (FR-2.14). */
export function ThemeCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("appearance.theme")} icon="palette" wide>
      <ThemeEditor settings={settings} update={update} />
    </Group>
  );
}

/** Visual effects ("website look") and the chat message layout (FR-1.48a, QA.25). */
export function LookCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("appearance.look")} icon="layout">
      <Switch
        label={t("appearanceEffects.visualEffects")}
        description={t("appearanceEffects.visualEffectsHint")}
        checked={settings.visualEffects}
        onChange={(visualEffects) => {
          update({ visualEffects });
        }}
      />
      <RadioGroup
        legend={t("appearance.messageDisplayLegend")}
        value={settings.chatDisplay}
        options={[
          { value: "cozy", label: t("appearance.cozy") },
          { value: "compact", label: t("appearance.compact") },
        ]}
        onChange={(chatDisplay) => {
          update({ chatDisplay });
        }}
      />
    </Group>
  );
}

export function ChannelListCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Group title={t("channelList.title")} icon="list">
      <Switch
        label={t("channelList.hideEmpty")}
        checked={settings.hideEmptyChannels}
        onChange={(hideEmptyChannels) => {
          update({ hideEmptyChannels });
        }}
      />
      <Switch
        label={t("channelList.talkingFirst")}
        checked={settings.talkingFirst}
        onChange={(talkingFirst) => {
          update({ talkingFirst });
        }}
      />
      <Switch
        label={t("channelList.weakConnection")}
        description={t("channelList.weakConnectionHint")}
        checked={settings.showWeakConnectionIcon}
        onChange={(showWeakConnectionIcon) => {
          update({ showWeakConnectionIcon });
        }}
      />
    </Group>
  );
}

/**
 * What closing the window does while connected (FR-1.38), starting at login (FR-1.48d) and, for
 * the AppImage, the app-menu entry.
 */
export function StartupCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const info = useAppInfo();
  return (
    <Group title={t("appearance.behaviour")} icon="power">
      <RadioGroup
        legend={t("appearance.closeLegend")}
        value={settings.closeBehaviour}
        options={[
          { value: "ask", label: t("appearance.closeAsk") },
          { value: "minimizeToTray", label: t("appearance.closeTray") },
          { value: "quit", label: t("appearance.closeQuit") },
        ]}
        onChange={(closeBehaviour) => {
          update({ closeBehaviour });
        }}
      />
      <Switch
        label={t("appearance.autostart")}
        checked={settings.autostart}
        onChange={(autostart) => {
          update({ autostart });
        }}
      />
      <Switch
        label={t("appearance.startMinimized")}
        description={t("appearance.startMinimizedHint")}
        checked={settings.startMinimized}
        disabled={!settings.autostart}
        onChange={(startMinimized) => {
          update({ startMinimized });
        }}
      />
      {info?.appimage === true && <AppImageMenuEntrySwitch />}
    </Group>
  );
}
