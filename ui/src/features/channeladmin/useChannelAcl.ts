import { useCallback, useState } from "react";
import { getApi, toEngineError, type ChannelAclView, type EngineError } from "../../api";

export type AclLoad =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "loaded"; acl: ChannelAclView }
  | { status: "failed"; error: EngineError };

/**
 * A channel's rules and groups for the Password and Permissions tabs (FR-10.36): read when one of
 * them first opens (the caller asks), again on "Refresh" or "Reload", and dropped with the window (FR-10.63).
 */
export function useChannelAcl(serverId: number | null, channelId: number | null) {
  const [load, setLoad] = useState<AclLoad>({ status: "idle" });

  const read = useCallback(async () => {
    if (serverId === null || channelId === null) return;
    setLoad({ status: "loading" });
    try {
      setLoad({ status: "loaded", acl: await getApi().channelAclRead(serverId, channelId) });
    } catch (error) {
      console.error("Reading the channel rules failed", error);
      setLoad({ status: "failed", error: toEngineError(error) });
    }
  }, [serverId, channelId]);

  const replace = useCallback((acl: ChannelAclView) => {
    setLoad({ status: "loaded", acl });
  }, []);

  return { load, reload: read, replace };
}
