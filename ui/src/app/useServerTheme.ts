import { useEffect } from "react";
import { useShownServerId } from "../features/channels";
import { useServers } from "../features/servers";
import { setServerThemeId } from "../theme/themeStore";

/** Shows the shown server's own theme while it is shown (FR-2.17); other screens use the app theme. */
export function useServerTheme(): void {
  const shownId = useShownServerId();
  const { servers } = useServers();
  const themeId = servers.find((server) => server.id === shownId)?.themeId ?? null;
  useEffect(() => {
    setServerThemeId(themeId);
  }, [themeId]);
  useEffect(
    () => () => {
      setServerThemeId(null);
    },
    [],
  );
}
