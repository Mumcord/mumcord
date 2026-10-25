import { useEffect, useState } from "react";
import { getApi, type PositionalStatus } from "../../api";

/** The positional status (FR-1.43): which source delivers positions now; `null` until loaded. */
export function usePositionalStatus(): PositionalStatus | null {
  const [status, setStatus] = useState<PositionalStatus | null>(null);
  useEffect(() => {
    let active = true;
    const api = getApi();
    api.positionalStatus().then(
      (loaded) => {
        if (active) setStatus((current) => current ?? loaded);
      },
      (error: unknown) => {
        console.error("Reading the positional status failed", error);
      },
    );
    const unlisten = api.on("positionalStatusChanged", (event) => {
      if (active) setStatus(event.status);
    });
    return () => {
      active = false;
      unlisten();
    };
  }, []);
  return status;
}
