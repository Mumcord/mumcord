import { useEffect } from "react";
import { getApi } from "../../api";
import { getVoiceServerId, subscribeAllServerChanges } from "../channels";
import { ducker, someoneTalking } from "./ducking";

/**
 * Feeds the ducker from the voice server's talking state and hands its level to the engine for
 * watched screen-share sound (FR-2.76). Renders nothing; the party player follows the ducker
 * itself.
 */
export function VideoDucking() {
  useEffect(() => {
    const unsubscribeServers = subscribeAllServerChanges((serverId, _change, state) => {
      const voice = getVoiceServerId();
      if (voice === null) ducker.setTalking(false);
      else if (serverId === voice) ducker.setTalking(someoneTalking(state));
    });
    const unsubscribeLevel = ducker.subscribe((level) => {
      getApi()
        .screenShareSetSoundLevel(level)
        .catch((error: unknown) => {
          console.warn("Setting the screen share sound level failed", error);
        });
    });
    return () => {
      unsubscribeServers();
      unsubscribeLevel();
    };
  }, []);
  return null;
}
