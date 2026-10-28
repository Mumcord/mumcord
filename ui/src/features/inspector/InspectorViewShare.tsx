import { useEffect, useMemo, useRef, useState } from "react";
import { getApi } from "../../api";
import { useServers as useConnectedServers } from "../channels/serverStore";
import { useServers as useSavedServers } from "../servers/store";
import { readRootLook, sharedServers, sharedUsers, type RootLook } from "./inspectorView";

/** The main window's root look, updated whenever its theme, hints or language change. */
function useRootLook(): RootLook {
  const [look, setLook] = useState(() => readRootLook(document.documentElement));
  useEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      setLook(readRootLook(root));
    });
    observer.observe(root, { attributes: true });
    return () => {
      observer.disconnect();
    };
  }, []);
  return look;
}

/**
 * Shares the main window's theme, language and known names with the packet inspector window
 * while the developer tools are unlocked. Its own component, because the connected servers
 * change with every talk state and must not re-render the main screen.
 */
export function InspectorViewShare() {
  const look = useRootLook();
  const connected = useConnectedServers();
  const saved = useSavedServers().servers;
  const users = useMemo(() => sharedUsers(connected), [connected]);
  const servers = useMemo(() => sharedServers(saved), [saved]);
  const last = useRef("");

  useEffect(() => {
    const view = { ...look, users, servers };
    const key = JSON.stringify(view);
    if (key === last.current) return;
    last.current = key;
    getApi()
      .packetInspectorShare(view)
      .catch((error: unknown) => {
        console.error("Sharing the view with the packet inspector failed", error);
      });
  }, [look, users, servers]);

  return null;
}
