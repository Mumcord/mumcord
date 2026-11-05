import type { PluginWebviewPlace } from "../../../api";
import { usePluginUi, type PluginWebview } from "../render/pluginUiStore";

/** The plugin's web view if it is open in `place`. */
export function usePluginWebView(
  plugin: string,
  place: PluginWebviewPlace,
): PluginWebview | undefined {
  return usePluginUi().webviews.get(plugin)?.get(place);
}
