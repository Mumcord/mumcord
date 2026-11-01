import { useEffect } from "react";
import { getApi } from "../../api";
import { usePreferences } from "../../app/preferences";
import { PluginDialogHost } from "./dialog";
import { usePluginDeepLinks } from "./browse/deepLink";
import { PluginsPalette } from "./PluginsPalette";
import { PluginToasts } from "./prompt/PluginToasts";
import { usePluginRequestRouter } from "./pluginRequestStore";
import { usePluginMentions } from "./chat";

const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";

const prefersReducedMotion = () =>
  typeof window.matchMedia === "function" && window.matchMedia(REDUCED_MOTION).matches;

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

  useEffect(() => {
    const send = () => {
      getApi()
        .pluginsSetAppearance(language, dark, prefersReducedMotion())
        .catch((error: unknown) => {
          console.error("Sending the appearance to the plugins failed", error);
        });
    };
    send();
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(REDUCED_MOTION);
    query.addEventListener("change", send);
    return () => {
      query.removeEventListener("change", send);
    };
  }, [language, dark]);

  return (
    <>
      <PluginsPalette />
      <PluginDialogHost />
      <PluginToasts />
    </>
  );
}
