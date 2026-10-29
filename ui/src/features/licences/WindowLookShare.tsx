import { useEffect, useRef } from "react";
import { getApi } from "../../api";
import { useRootLook } from "../inspector/useRootLook";

/**
 * Shares the main window's theme and language with the third-party licences (issue #138) and
 * "What's new" (issue #145) windows, which follow them live while they are open.
 */
export function WindowLookShare() {
  const look = useRootLook();
  const last = useRef("");

  useEffect(() => {
    const key = JSON.stringify(look);
    if (key === last.current) return;
    last.current = key;
    getApi()
      .windowLookShare(look)
      .catch((error: unknown) => {
        console.error("Sharing the look with the other windows failed", error);
      });
  }, [look]);

  return null;
}
