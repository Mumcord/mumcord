import { useEffect, useState } from "react";
import { getApi, type P2pLink } from "../../api";

/**
 * The open P2P connection to `session` on `serverId` (FR-4.19), or `null` while there is none.
 * Read once and kept current by `p2pLinkChanged`.
 */
export function useP2pLink(serverId: number | null, session: number): P2pLink | null {
  // Keyed by person, so a value of the previously shown person is never returned.
  const [state, setState] = useState<{ key: string; link: P2pLink | null } | null>(null);
  const key = `${String(serverId)}:${String(session)}`;
  useEffect(() => {
    if (serverId === null) return;
    const setLink = (link: P2pLink | null) => {
      setState({ key, link });
    };
    const api = getApi();
    let active = true;
    let fromEvent = false;
    const unsubscribe = api.on("p2pLinkChanged", (event) => {
      if (event.serverId !== serverId || event.session !== session) return;
      fromEvent = true;
      setLink(event.link);
    });
    api.p2pLink(serverId, session).then(
      (read) => {
        if (active && !fromEvent) setLink(read);
      },
      (error: unknown) => {
        console.error("Reading the P2P connection failed", error);
      },
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [serverId, session, key]);
  return state?.key === key ? state.link : null;
}
