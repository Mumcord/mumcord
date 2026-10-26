import { useTranslation } from "react-i18next";
import { RadioGroup } from "../components/RadioGroup";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { ThemeEditor } from "../theme/ThemeEditor";
import type { SettingsSectionProps } from "./types";

/**
 * Appearance: colour theme and its editor (FR-2.14), chat message layout (FR-1.48a, QA.25), plus window behaviour: what closing the window does while
 * connected (FR-1.38) and starting at login (FR-1.48d).
 */
export function AppearanceSection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.appearance")}>
      <Group title={t("appearance.theme")}>
        <ThemeEditor settings={settings} update={update} />
      </Group>
      <Group title={t("appearance.messageDisplay")}>
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
      <Group title={t("appearanceEffects.title")}>
        <Switch
          label={t("appearanceEffects.visualEffects")}
          description={t("appearanceEffects.visualEffectsHint")}
          checked={settings.visualEffects}
          onChange={(visualEffects) => {
            update({ visualEffects });
          }}
        />
      </Group>
      <Group title={t("channelList.title")}>
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
      <Group title={t("appearance.behaviour")}>
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
      </Group>
    </Section>
  );
}
