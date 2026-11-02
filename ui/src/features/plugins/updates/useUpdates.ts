import { useCallback, useEffect, useState } from "react";
import { getApi, type PluginUpdate } from "../../../api";

export interface UpdatesState {
  /** `null` until the first load has finished. */
  updates: PluginUpdate[] | null;
  loadFailed: boolean;
  reload: () => Promise<void>;
}

async function fetchUpdates(): Promise<PluginUpdate[] | null> {
  try {
    return await getApi().pluginUpdates();
  } catch (error: unknown) {
    console.error("Loading the plugin updates failed", error);
    return null;
  }
}

/** The updates on offer; the engine sends `plugins://changed` whenever they change. */
export function useUpdates(): UpdatesState {
  const [updates, setUpdates] = useState<PluginUpdate[] | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);

  const apply = useCallback((loaded: PluginUpdate[] | null) => {
    if (loaded !== null) setUpdates(loaded);
    setLoadFailed(loaded === null);
  }, []);

  const reload = useCallback(async () => {
    apply(await fetchUpdates());
  }, [apply]);

  useEffect(() => {
    let active = true;
    const load = () => {
      void fetchUpdates().then((loaded) => {
        if (active) apply(loaded);
      });
    };
    load();
    const off = getApi().on("pluginsChanged", load);
    return () => {
      active = false;
      off();
    };
  }, [apply]);

  return { updates, loadFailed, reload };
}
