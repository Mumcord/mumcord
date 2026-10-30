import { useTranslation } from "react-i18next";
import { shownServerId } from "../channels/serverStore";
import { useServers } from "./store";

/**
 * "Identity: ‹label›" pill (issue #148). Shown next to the conversation title only while the
 * shown server connects with a non-default identity, so the normal case stays quiet.
 */
export function IdentityBadge({ className }: { className?: string }) {
  const { t } = useTranslation("servers");
  const state = useServers();
  const server = state.servers.find((s) => s.id === shownServerId());
  if (!server || server.identityId === null) return null;
  const identity = state.identities.find((i) => i.id === server.identityId);
  if (!identity) return null;
  return (
    <span className={className} aria-label={t("identity.badgeTitle", { identity: identity.label })}>
      {t("identity.badge", { identity: identity.label })}
    </span>
  );
}
