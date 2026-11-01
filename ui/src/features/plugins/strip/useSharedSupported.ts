import { useEffect, useState } from "react";
import { getApi } from "../../../api";
import { useServerSelector } from "../../channels/serverStore";

/**
 * Whether the shown server passes plugin data (FR-6.38). The engine decides after connecting, so
 * the answer is asked once and then follows `plugins://shared-state`. Unknown counts as
 * supported: the hint must not flash while the engine is still deciding.
 */
export function useSharedSupported(): boolean {
  const serverId = useServerSelector((s) => s.serverId);
  const [state, setState] = useState<{ serverId: number; supported: boolean } | null>(null);

  useEffect(() => {
    if (serverId === null) return;
    const api = getApi();
    let live = true;
    const unlisten = api.on("pluginsSharedState", (event) => {
      if (live && event.serverId === serverId) setState({ serverId, supported: event.supported });
    });
    api.pluginsSharedSupported(serverId).then(
      (supported) => {
        if (live) setState({ serverId, supported });
      },
      () => undefined,
    );
    return () => {
      live = false;
      unlisten();
    };
  }, [serverId]);

  return state === null || state.serverId !== serverId ? true : state.supported;
}
