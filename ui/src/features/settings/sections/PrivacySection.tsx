import { useTranslation } from "react-i18next";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { AcceptedConversationsGroup } from "./AcceptedConversationsGroup";
import { BlockedMessagesGroup, BlockedUsersGroup } from "./BlockedUsersGroup";
import { CacheGroup } from "./CacheGroup";
import { PrivateMessagesGroup } from "./PrivateMessagesGroup";
import type { SettingsSectionProps } from "./types";

/**
 * Privacy: blocked users (issue #140), who may send private messages (issue #141), consent to direct connections between
 * Mumcord users (FR-2.36) and the caches.
 */
export function PrivacySection({ settings, update }: SettingsSectionProps) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.privacy")}>
      <BlockedUsersGroup />
      <BlockedMessagesGroup settings={settings} update={update} />
      <PrivateMessagesGroup settings={settings} update={update} />
      <AcceptedConversationsGroup />
      <Group title={t("privacyDirect.title")}>
        <Switch
          label={t("privacyDirect.allow")}
          description={t("privacyDirect.hint")}
          checked={settings.allowDirectConnections === "yes"}
          onChange={(allowed) => {
            update({ allowDirectConnections: allowed ? "yes" : "no" });
          }}
        />
      </Group>
      <CacheGroup />
    </Section>
  );
}
