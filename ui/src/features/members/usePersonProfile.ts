/**
 * Loads a person's profile (avatar, comment) once per session; shared by the profile card and the
 * hover preview. `loadError` is set when the engine call failed.
 */
import { useEffect, useState } from "react";
import { getApi, toEngineError, type PersonProfile } from "../../api";
import { shownServerId } from "../channels/serverStore";

export function usePersonProfile(
  session: number,
  serverId?: number,
): {
  profile: PersonProfile | null;
  loadError: boolean;
} {
  const [loaded, setLoaded] = useState<{
    session: number;
    profile: PersonProfile | null;
    loadError: boolean;
  } | null>(null);

  useEffect(() => {
    let active = true;
    getApi()
      .personProfile(serverId ?? shownServerId(), session)
      .then((profile) => {
        if (active) setLoaded({ session, profile, loadError: false });
      })
      .catch((cause: unknown) => {
        console.error("Loading a profile failed", toEngineError(cause).code);
        if (active) setLoaded({ session, profile: null, loadError: true });
      });
    return () => {
      active = false;
    };
  }, [session, serverId]);

  // A result for a previous session (the preview switched people) is not shown.
  return loaded?.session === session
    ? { profile: loaded.profile, loadError: loaded.loadError }
    : { profile: null, loadError: false };
}
