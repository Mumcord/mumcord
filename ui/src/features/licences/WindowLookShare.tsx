import { useEffect, useRef } from "react";
import { getApi } from "../../api";
import { useRootLook } from "../inspector/useRootLook";

/**
 * Shares the main window's theme and language with the third-party licences window (issue
 * #138), which follows them live while it is open.
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
        console.error("Sharing the look with the licences window failed", error);
      });
  }, [look]);

  return null;
}
