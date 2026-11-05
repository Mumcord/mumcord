import { useEffect, useState } from "react";
import { getApi, type PluginRelayAvailability } from "../../../api";

interface Known {
  serverId: number;
  availability: PluginRelayAvailability;
}

/**
 * Whether `serverId`'s relay can carry plugin data (FR-9.57), kept current from the engine's
 * `pluginsRelayAvailability` event. `null` until the first answer, or while no server is shown.
 */
export function useRelayAvailability(serverId: number | null): PluginRelayAvailability | null {
  const [known, setKnown] = useState<Known | null>(null);
  useEffect(() => {
    if (serverId === null) return;
    let active = true;
    const api = getApi();
    const unsubscribe = api.on("pluginsRelayAvailability", (change) => {
      if (active && change.serverId === serverId) {
        setKnown({ serverId, availability: change.availability });
      }
    });
    api.pluginRelayAvailability(serverId).then(
      (availability) => {
        if (active) setKnown((current) => current ?? { serverId, availability });
      },
      (error: unknown) => {
        console.error("Loading the relay availability failed", error);
      },
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [serverId]);
  return known !== null && known.serverId === serverId ? known.availability : null;
}
