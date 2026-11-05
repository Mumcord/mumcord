import { useEffect, useState } from "react";
import { getApi } from "../../../api";

const REFRESH_MS = 5000;

/** Plugin data each plugin moved through relays this session (FR-9.64), by plugin id. */
export function useRelayUsage(): ReadonlyMap<string, number | null> {
  const [usage, setUsage] = useState<ReadonlyMap<string, number | null>>(new Map());
  useEffect(() => {
    let active = true;
    const load = () => {
      getApi()
        .pluginRelayUsage()
        .then(
          (rows) => {
            if (active) setUsage(new Map(rows.map((row) => [row.pluginId, row.bytes])));
          },
          (error: unknown) => {
            console.error("Loading the relay data usage failed", error);
          },
        );
    };
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);
  return usage;
}
