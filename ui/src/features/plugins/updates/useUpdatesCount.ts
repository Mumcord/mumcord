import { usePlugins } from "../pluginStore";
import { useUpdates } from "./useUpdates";

/** Waiting updates plus plugins with new optional permissions to answer (F39). */
export function useUpdatesCount(): number {
  const { overview } = usePlugins();
  const { updates } = useUpdates();
  return (
    (updates?.length ?? 0) + (overview?.plugins.filter((p) => p.offered.length > 0).length ?? 0)
  );
}
