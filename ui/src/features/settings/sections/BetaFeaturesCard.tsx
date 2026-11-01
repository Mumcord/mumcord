import { useTranslation } from "react-i18next";
import { BetaBadge } from "../../../components/BetaBadge";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import { useSettingsNavigation } from "../settingsNavigation";
import type { SettingsSectionProps } from "./types";

/**
 * Every Beta switch in one place (#161). Each one is bound to the same setting as its control in
 * context, which keeps its Beta badge there; game plugins have no switch, so the row leads to them.
 */
export function BetaFeaturesCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  const navigation = useSettingsNavigation();
  return (
    <Group title={t("betaFeatures.title")} icon="flask" info={t("betaFeatures.hint")} wide>
      <Switch
        label={t("positional.switch")}
        badge={<BetaBadge />}
        description={t("positional.hint")}
        checked={settings.positional.enabled}
        onChange={(enabled) => {
          update({ positional: { ...settings.positional, enabled } });
        }}
      />
      <Switch
        label={t("fullImages.offer")}
        badge={<BetaBadge />}
        description={t("fullImages.offerHint")}
        checked={settings.offerFullImages}
        onChange={(offerFullImages) => {
          update({ offerFullImages });
        }}
      />
      <Switch
        label={t("fullImages.load")}
        badge={<BetaBadge />}
        description={t("fullImages.loadHint")}
        checked={settings.loadFullImages}
        onChange={(loadFullImages) => {
          update({ loadFullImages });
        }}
      />
      <Switch
        label={t("audio.lowLatency")}
        badge={<BetaBadge />}
        description={t("audio.lowLatencyHint")}
        checked={settings.lowLatency}
        onChange={(lowLatency) => {
          update({ lowLatency });
        }}
      />
      <div className="settings-control-row">
        <span className="beta-features__label">
          {t("betaFeatures.gamePlugins")}
          <BetaBadge />
        </span>
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            navigation?.jumpTo({ section: "gaming", group: "gamePlugins.title" });
          }}
        >
          {t("betaFeatures.manage")}
        </button>
      </div>
    </Group>
  );
}
