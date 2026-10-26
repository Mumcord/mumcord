import { useEffect, useState } from "react";
import { getApi } from "../../../api";

let version: Promise<string> | null = null;

/**
 * The running Mumcord version from the backend, which theme codes carry (FR-2.16); `null` until
 * it is known. Asked once per app run.
 */
export function useAppVersion(): string | null {
  const [current, setCurrent] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    version ??= getApi()
      .appInfo()
      .then((info) => info.version);
    version.then(
      (value) => {
        if (active) setCurrent(value);
      },
      (error: unknown) => {
        console.error("Reading the app version failed", error);
        version = null;
      },
    );
    return () => {
      active = false;
    };
  }, []);
  return current;
}
