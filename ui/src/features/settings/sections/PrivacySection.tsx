import { useTranslation } from "react-i18next";
import { P2pModeRadios } from "../../p2p/P2pModeRadios";
import { Group } from "../components/Section";
import { Switch } from "../components/Switch";
import type { SettingsSectionProps } from "./types";

/**
 * The peer-to-peer mode (spec 04, FR-4.1) and consent to direct connections between Mumcord
 * users (FR-2.36).
 */
export function ConnectionsCard({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  // These modes never connect directly, so the consent has no effect (spec 04, FR-4.3).
  const directUnused = settings.p2pMode === "relayOnly" || settings.p2pMode === "off";
  return (
    <Group title={t("privacyConnections.title")} icon="share">
      <P2pModeRadios
        value={settings.p2pMode}
        hideTitle
        onChange={(mode) => {
          if (mode !== null) update({ p2pMode: mode });
        }}
      />
      <Switch
        label={t("privacyDirect.allow")}
        description={directUnused ? t("privacyDirect.notUsed") : t("privacyDirect.hint")}
        checked={settings.allowDirectConnections === "yes"}
        disabled={directUnused}
        onChange={(allowed) => {
          update({ allowDirectConnections: allowed ? "yes" : "no" });
        }}
      />
    </Group>
  );
}
