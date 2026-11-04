/**
 * The header Mumcord draws above a plugin activity, outside the plugin's content (FR-9.48,
 * mockup surfaces 03): icon and name, who started it and how many are in, who made the plugin,
 * the "Unofficial" chip and the connection chip.
 */
import { useTranslation } from "react-i18next";
import type { InstalledPlugin, StageActivityView } from "../../../api";
import { StageConnection } from "./StageConnection";

/** The plugin's icon, or its first letter when it has none. */
export function StageIcon({
  name,
  url,
  small = false,
}: {
  name: string;
  url: string | null;
  small?: boolean;
}) {
  return (
    <span className="plugin-stage__icon" data-small={small || undefined} aria-hidden="true">
      {url === null ? (Array.from(name)[0] ?? "").toUpperCase() : <img src={url} alt="" />}
    </span>
  );
}

/** Who made the plugin: its repository for unofficial plugins, nothing for official ones. */
function makerOf(plugin: InstalledPlugin | undefined): string | null {
  if (plugin?.source.kind !== "repo" || plugin.source.name === "") return null;
  return plugin.source.name;
}

interface StageHeaderProps {
  activity: StageActivityView;
  plugin: InstalledPlugin | undefined;
  icon: string | null;
  /** The server whose relay the connection chip describes. */
  serverId: number | null;
}

export function StageHeader({ activity, plugin, icon, serverId }: StageHeaderProps) {
  const { t } = useTranslation("pluginStage");
  const name = activity.pluginName || activity.plugin;
  const count = activity.participants.length;
  const started = activity.startedByMe
    ? t("startedByYou", { count })
    : t("startedBy", { name: activity.starterName, count });
  const maker = makerOf(plugin);
  // An activity always comes from an installed plugin; while the list loads it counts as official.
  const unofficial = plugin !== undefined && plugin.source.kind !== "official";
  return (
    <header className="plugin-stage__head">
      <StageIcon name={name} url={icon} />
      <span className="plugin-stage__titles">
        <span className="plugin-stage__title">{activity.title || name}</span>
        <span className="plugin-stage__sub">
          {maker === null ? started : `${started} · ${t("pluginBy", { author: maker })}`}
        </span>
      </span>
      <span className="plugin-stage__chips">
        {unofficial && (
          <span className="plugin-stage__chip" data-tone="unofficial">
            {t("unofficial")}
          </span>
        )}
        <StageConnection serverId={serverId} plugin={plugin} />
      </span>
    </header>
  );
}
