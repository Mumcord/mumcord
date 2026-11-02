import { useEffect, useRef } from "react";
import { getApi } from "../../../api";
import { usePreferences } from "../../../app/preferences";
import { routePluginRequest } from "../pluginRequestStore";
import { loadPluginIndex } from "./indexStore";

/**
 * `mumcord://plugins/<id>` opens the details of an official plugin in Browse (FR-6.65). Ids that
 * are not in the official list do nothing; a link never installs. Mounted once by PluginsHost.
 */
export function usePluginDeepLinks(): void {
  const { language } = usePreferences();
  const languageRef = useRef(language);
  useEffect(() => {
    languageRef.current = language;
  }, [language]);

  useEffect(() => {
    let active = true;
    const open = (id: string) => {
      void loadPluginIndex(languageRef.current).then((index) => {
        const official = index?.plugins.some(
          (plugin) => plugin.id === id && plugin.source.kind === "official",
        );
        if (active && official === true) routePluginRequest({ kind: "details", plugin: id });
      });
    };
    const api = getApi();
    const unsubscribe = api.on("pluginsOpenDetails", ({ id }) => {
      open(id);
    });
    // A link that launched the app arrived before this listener; take it once, after subscribing.
    api
      .pluginsTakePendingLink()
      .then((id) => {
        if (id !== null) open(id);
      })
      .catch((error: unknown) => {
        console.error("Reading the pending plugin link failed", error);
      });
    return () => {
      active = false;
      unsubscribe();
    };
  }, []);
}
