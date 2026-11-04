/**
 * The connection chip of the stage header (FR-9.48, FR-9.52). Plugin data goes over the Mumble
 * connection until relay plugin data exists (R3a); the relay data task (R4) replaces this chip
 * with the live state and adds the "No relay for plugin data" banner of mockup 08.
 */
import { useTranslation } from "react-i18next";

export function StageConnection() {
  const { t } = useTranslation("pluginStage");
  return (
    <span className="plugin-stage__chip" data-tone="warn">
      {t("connection.mumble")}
    </span>
  );
}
