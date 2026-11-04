import { useTranslation } from "react-i18next";
import { getApi, type InstalledPlugin, type PluginRelayAvailability } from "../../../api";
import { Group } from "../../settings/components/Section";
import { Switch } from "../../settings/components/Switch";
import { useServers } from "../../servers/store";
import { displayName, initials } from "../installed/labels";
import { updatePlatformSettings, usePlatformSettings } from "../platformSettings";
import { usePlugins } from "../pluginStore";
import { RELAY, asksForRelay } from "./relayPermission";
import { useRelayAvailability } from "./useRelayAvailability";
import { useRelayUsage } from "./useRelayUsage";
import "./relaydata.css";

const DOT_TONE: Record<PluginRelayAvailability["kind"], string> = {
  available: "ok",
  useNotAllowed: "warn",
  notAllowedHere: "warn",
  noRelay: "none",
};

function ServerRow({ serverId, label }: { serverId: number; label: string }) {
  const { t } = useTranslation("pluginRelay");
  const availability = useRelayAvailability(serverId);
  if (availability === null) return null;
  return (
    <li>
      <span className="relay-dot" data-tone={DOT_TONE[availability.kind]} aria-hidden="true" />
      <span>
        {availability.kind === "available"
          ? t("serverAllows", { server: label })
          : availability.kind === "noRelay"
            ? t("serverNone", { server: label })
            : t("serverDenies", { server: label })}
      </span>
      <span className="relay-usage">
        {availability.kind === "available"
          ? availability.kbps === null
            ? ""
            : t("serverAllowsRate", { rate: availability.kbps })
          : t("mumbleOnly")}
      </span>
    </li>
  );
}

/** "3.4 MB": decimal units, as on the mockup and in the operating system's file managers. */
function formatSize(bytes: number, language: string): string {
  const [unit, divisor] =
    bytes >= 1_000_000
      ? (["megabyte", 1_000_000] as const)
      : bytes >= 1000
        ? (["kilobyte", 1000] as const)
        : (["byte", 1] as const);
  return new Intl.NumberFormat(language, {
    style: "unit",
    unit,
    unitDisplay: "short",
    maximumFractionDigits: unit === "byte" ? 0 : 1,
  }).format(bytes / divisor);
}

function PluginRow({ plugin, bytes }: { plugin: InstalledPlugin; bytes: number | null }) {
  const { t, i18n } = useTranslation("pluginRelay");
  const name = displayName(plugin);
  const official = plugin.source.kind === "official";
  const asks = asksForRelay(plugin);
  const granted = plugin.granted.includes(RELAY);
  const setGranted = (on: boolean) => {
    const next = on ? [...plugin.granted, RELAY] : plugin.granted.filter((id) => id !== RELAY);
    getApi()
      .pluginSetPermissions(plugin.id, next)
      .catch((error: unknown) => {
        console.error("Changing the relay permission failed", error);
      });
  };
  const size = formatSize(bytes ?? 0, i18n.language);
  return (
    <li className="relay-plugin">
      <span className="relay-plugin__icon" aria-hidden="true">
        {initials(name)}
      </span>
      <span className="relay-plugin__text">
        {name}
        <small>
          {asks ? t(official ? "pluginUsedOfficial" : "pluginUsed", { size }) : t("officialNone")}
        </small>
      </span>
      {asks && (
        <input
          type="checkbox"
          role="switch"
          className="settings-switch__input"
          checked={granted}
          aria-label={t("mayUse", { plugin: name })}
          onChange={(event) => {
            setGranted(event.target.checked);
          }}
        />
      )}
    </li>
  );
}

/**
 * "Plugin data through the relay" (FR-9.64, surfaces mockup 07): the global switch, the relay
 * status of each saved server and the data each plugin used this session.
 */
export function RelayDataGroup() {
  const { t } = useTranslation("pluginRelay");
  const settings = usePlatformSettings();
  const { servers } = useServers();
  const { overview } = usePlugins();
  const usage = useRelayUsage();
  const plugins = (overview?.plugins ?? []).filter(
    (plugin) => asksForRelay(plugin) || plugin.source.kind === "official",
  );
  return (
    <Group title={t("title")}>
      <Switch
        label={t("switchLabel")}
        checked={settings?.relayDataEnabled ?? true}
        disabled={settings === null}
        status={t("switchText")}
        onChange={(relayDataEnabled) => void updatePlatformSettings({ relayDataEnabled })}
      />
      <p className="relay-label">{t("servers")}</p>
      <ul className="relay-servers" aria-label={t("servers")}>
        {servers.map((server) => (
          <ServerRow key={server.id} serverId={server.id} label={server.label} />
        ))}
      </ul>
      <p className="relay-label">{t("plugins")}</p>
      {plugins.length === 0 ? (
        <p className="plugins-safety-note">{t("noPlugins")}</p>
      ) : (
        <ul className="relay-plugins" aria-label={t("plugins")}>
          {plugins.map((plugin) => (
            <PluginRow key={plugin.id} plugin={plugin} bytes={usage.get(plugin.id) ?? null} />
          ))}
        </ul>
      )}
    </Group>
  );
}
