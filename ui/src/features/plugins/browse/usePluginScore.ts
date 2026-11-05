import { useEffect, useState } from "react";
import { getApi } from "../../../api";
import type { PluginScored } from "../../../bindings/commands";

/** The engine's score for one version; `null` while loading, for official plugins and on failure. */
export function usePluginScore(
  id: string,
  version: string,
  from: "listing" | "package",
  official: boolean,
): PluginScored | null {
  const [scored, setScored] = useState<PluginScored | null>(null);
  useEffect(() => {
    if (official) return;
    let active = true;
    getApi()
      .pluginScore(id, version, from)
      .then(
        (result) => {
          if (active) setScored(result);
        },
        (error: unknown) => {
          console.error("Loading the security score failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, [id, version, from, official]);
  return scored;
}
