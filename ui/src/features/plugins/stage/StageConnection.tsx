/**
 * How the activity's data travels (FR-9.48, FR-9.52, surfaces mockups 03 and 08): the shared relay
 * data chip in the stage header, and the "No relay for plugin data on this server." banner under
 * it while the server's relay cannot carry plugin data.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { InstalledPlugin } from "../../../api";
import { useServerStateOf } from "../../channels";
import { useInPopout } from "../../media/popoutStore";
import { useServers } from "../../servers/store";
import { usePlatformSettings } from "../platformSettings";
import { openPluginTab } from "../pluginRequestStore";
import { RELAY, RelayDataChip, asksForRelay, useRelayAvailability } from "../relaydata";

/**
 * Whether the relay may carry this plugin's data once the server allows it: the plugin asks for
 * the relay, the user granted it and "Plugin data through the relay" is on (FR-9.63, FR-9.64).
 */
function useRelayUsable(plugin: InstalledPlugin | undefined): boolean {
  const settings = usePlatformSettings();
  if (plugin === undefined || !asksForRelay(plugin) || !plugin.granted.includes(RELAY))
    return false;
  return settings?.relayDataEnabled ?? true;
}

export function StageConnection({
  serverId,
  plugin,
}: {
  serverId: number | null;
  plugin: InstalledPlugin | undefined;
}) {
  return <RelayDataChip serverId={serverId} usable={useRelayUsable(plugin)} />;
}

/** The banner of mockup 08; nothing while the relay can carry plugin data or the state is unknown. */
export function NoRelayBanner({ serverId, plugin }: { serverId: number | null; plugin: string }) {
  const availability = useRelayAvailability(serverId);
  const inPopout = useInPopout();
  if (serverId === null || availability === null || availability.kind === "available") return null;
  // The pop-out window cannot read the saved servers (nor open the settings), so it names the
  // server after its root channel and leaves "Details" to the main window.
  return inPopout ? (
    <PopoutBanner serverId={serverId} plugin={plugin} />
  ) : (
    <MainBanner serverId={serverId} plugin={plugin} />
  );
}

function MainBanner({ serverId, plugin }: { serverId: number; plugin: string }) {
  const { t } = useTranslation("pluginStage");
  const server = useServers().servers.find((s) => s.id === serverId)?.label ?? "";
  return (
    <Banner server={server} plugin={plugin}>
      <button
        type="button"
        className="plugin-stage__button plugin-stage__banner-button"
        onClick={() => {
          openPluginTab("installed");
        }}
      >
        {t("noRelayDetails")}
      </button>
    </Banner>
  );
}

function PopoutBanner({ serverId, plugin }: { serverId: number; plugin: string }) {
  const server = useServerStateOf(serverId).channels.get(0)?.name ?? "";
  return <Banner server={server} plugin={plugin} />;
}

function Banner({
  server,
  plugin,
  children,
}: {
  server: string;
  plugin: string;
  children?: ReactNode;
}) {
  const { t } = useTranslation("pluginStage");
  return (
    <div className="plugin-stage__relay" role="status">
      <WarningIcon />
      <span>
        <strong>{t("noRelayTitle")}</strong> {t("noRelayText", { server, plugin })}
      </span>
      {children}
    </div>
  );
}

function WarningIcon() {
  return (
    <svg
      className="plugin-stage__relay-icon"
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" />
      <path d="M12 9v4M12 17h.01" />
    </svg>
  );
}
