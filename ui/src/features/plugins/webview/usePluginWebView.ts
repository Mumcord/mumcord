import type { PluginWebviewPlace } from "../../../api";
import { usePluginUi, type PluginWebview } from "../render/pluginUiStore";

/** The plugin's web view if it is open in `place`. */
export function usePluginWebView(
  plugin: string,
  place: PluginWebviewPlace,
): PluginWebview | undefined {
  const webview = usePluginUi().webviews.get(plugin);
  return webview?.place === place ? webview : undefined;
}
