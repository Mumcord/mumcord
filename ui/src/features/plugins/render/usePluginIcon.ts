import { usePlugins } from "../pluginStore";
import { usePluginIconUrl } from "./assets";

/** The manifest icon of an installed plugin by id (`null` for none, for a suggestion not yet installed, or while loading). */
export function usePluginIcon(plugin: string | null): string | null {
  const { overview } = usePlugins();
  const icon = overview?.plugins.find((p) => p.id === plugin)?.icon ?? null;
  return usePluginIconUrl(plugin ?? "", icon);
}
