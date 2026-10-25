import { useEffect, useState } from "react";
import { getApi } from ".";

/**
 * Whether the app runs on macOS, from the backend's OS description. macOS has no overlay at
 * all (FR-1.46b, QA.58), so the overlay UI is hidden there; until the answer arrives it is shown.
 */
export function useIsMacos(): boolean {
  const [macos, setMacos] = useState(false);
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (active) setMacos(info.os.startsWith("macos"));
        },
        (error: unknown) => {
          console.error("Reading the app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return macos;
}
