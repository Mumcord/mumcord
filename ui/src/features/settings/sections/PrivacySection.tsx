import { useTranslation } from "react-i18next";
import { P2pModeRadios } from "../../p2p/P2pModeRadios";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { AcceptedConversationsGroup } from "./AcceptedConversationsGroup";
import { CacheGroup } from "./CacheGroup";
import { PrivateMessagesGroup } from "./PrivateMessagesGroup";
import type { SettingsSectionProps } from "./types";

/**
 * Privacy: who may send private messages (issue #141), the peer-to-peer mode (spec 04, FR-4.1),
 * consent to direct connections between Mumcord users (FR-2.36) and the caches.
 */
export function PrivacySection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  // These modes never connect directly, so the consent has no effect (spec 04, FR-4.3).
  const directUnused = settings.p2pMode === "relayOnly" || settings.p2pMode === "off";
  return (
    <Section title={t("sections.privacy")}>
      <PrivateMessagesGroup settings={settings} update={update} />
      <AcceptedConversationsGroup />
      <Group title={t("p2p.title")}>
        <P2pModeRadios
          value={settings.p2pMode}
          hideTitle
          onChange={(mode) => {
            if (mode !== null) update({ p2pMode: mode });
          }}
        />
      </Group>
      <Group title={t("privacyDirect.title")}>
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
      <CacheGroup />
    </Section>
  );
}
