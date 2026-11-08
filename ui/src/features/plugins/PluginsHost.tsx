import { useEffect } from "react";
import { getApi, type PluginPageTheme } from "../../api";
import { usePreferences } from "../../app/preferences";
import { PluginDialogHost } from "./dialog";
import { usePluginDeepLinks } from "./browse/deepLink";
import { PluginsPalette } from "./PluginsPalette";
import { PluginToasts } from "./prompt/PluginToasts";
import { PluginStageHost } from "./stage";
import { usePluginFramesDeafen } from "./webview";
import { usePluginRequestRouter } from "./pluginRequestStore";
import { usePluginMentions } from "./chat";
import { onPageThemeChange, readPageTheme, reducedMotion } from "./render/pageTheme";

/**
 * Tells the plugins which language and colour scheme the app shows (plugins draw their UI in
 * them, B18), routes requests to open a plugin screen and hosts the command palette. Mount once
 * in the app shell.
 */
export function PluginsHost() {
  const { language, resolvedTheme } = usePreferences();
  const dark = resolvedTheme === "dark";
  usePluginRequestRouter();
  usePluginMentions();
  usePluginDeepLinks();
  usePluginFramesDeafen();

  useEffect(() => {
    let reduced = reducedMotion();
    const send = () => {
      getApi()
        .pluginsSetAppearance(language, dark, reduced)
        .catch((error: unknown) => {
          console.error("Sending the appearance to the plugins failed", error);
        });
    };
    send();
    return onPageThemeChange((theme) => {
      if (theme.reducedMotion === reduced) return;
      reduced = theme.reducedMotion;
      send();
    });
  }, [language, dark]);

  // API 1.2 pages served from now on start with the current theme (FR-13.13).
  useEffect(() => {
    const send = (theme: PluginPageTheme) => {
      getApi()
        .pluginsSetPageTheme(theme)
        .catch((error: unknown) => {
          console.error("Sending the theme for plugin pages failed", error);
        });
    };
    send(readPageTheme());
    return onPageThemeChange(send);
  }, []);

  return (
    <>
      <PluginsPalette />
      <PluginDialogHost />
      <PluginToasts />
      <PluginStageHost />
    </>
  );
}
