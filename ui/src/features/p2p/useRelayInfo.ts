import { useEffect, useState } from "react";
import { getApi, type RelayInfo } from "../../api";

/** The relay of `serverId` (FR-4.20), read once and kept current by `relayChanged`. */
export function useRelayInfo(serverId: number | null): RelayInfo | null {
  // Keyed by server, so a value of the previously shown server is never returned.
  const [state, setState] = useState<{ serverId: number; info: RelayInfo } | null>(null);
  useEffect(() => {
    if (serverId === null) return;
    const setInfo = (info: RelayInfo) => {
      setState({ serverId, info });
    };
    const api = getApi();
    let active = true;
    // An event may arrive before the first read answers; the event is newer then.
    let fromEvent = false;
    const unsubscribe = api.on("relayChanged", (event) => {
      if (event.serverId !== serverId) return;
      fromEvent = true;
      setInfo(event.info);
    });
    api.relayInfo(serverId).then(
      (read) => {
        if (active && !fromEvent) setInfo(read);
      },
      (error: unknown) => {
        console.error("Reading the relay info failed", error);
      },
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [serverId]);
  return state?.serverId === serverId ? state.info : null;
}
