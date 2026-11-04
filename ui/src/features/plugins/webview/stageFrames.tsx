/**
 * Puts every plugin page shown on the stage (`show(stage)`, FR-9.46) into the media area as the
 * plugin's activity tab, through the stage registry. The tab lives while the page is open; the
 * frame stays mounted while another tab is the main item.
 */
import { useEffect, useRef } from "react";
import { registerStageItem } from "../../media/stageRegistry";
import { usePlugins } from "../pluginStore";
import { usePluginUi } from "../render/pluginUiStore";
import { PluginFrame } from "./PluginFrame";
import { usePluginWebView } from "./usePluginWebView";

/** The stage frame of `plugin`, read live so a new page reaches the open tab. */
function StageFrame({ plugin }: { plugin: string }) {
  const webview = usePluginWebView(plugin, "stage");
  return webview ? <PluginFrame plugin={plugin} webview={webview} /> : null;
}

function StageEntry({ plugin, label }: { plugin: string; label: string }) {
  const unregister = useRef<(() => void) | null>(null);
  // Registering the same id again only renames the tab, so a new name never closes it.
  useEffect(() => {
    unregister.current = registerStageItem(`plugin:${plugin}`, {
      label,
      render: () => <StageFrame plugin={plugin} />,
    });
  }, [plugin, label]);
  useEffect(
    () => () => {
      unregister.current?.();
      unregister.current = null;
    },
    [plugin],
  );
  return null;
}

/** Registers the stage tabs of plugin pages. Mount once, in the main window. */
export function PluginStageFrames() {
  const { webviews } = usePluginUi();
  const plugins = usePlugins().overview?.plugins;
  return [...webviews.entries()]
    .filter(([, places]) => places.has("stage"))
    .map(([id]) => (
      <StageEntry key={id} plugin={id} label={plugins?.find((p) => p.id === id)?.name || id} />
    ));
}
