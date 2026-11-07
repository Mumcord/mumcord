/**
 * Whether the app runs on Linux, and its distribution when the engine names one (FR-2.41a).
 * What the system can play, and whether a player may run in the app's own window at all, depends
 * on it; the answer arrives with the app info, so it is `null` until then.
 */
import { useEffect, useState } from "react";
import { getApi, type LinuxDistro } from "../../api";

export interface LinuxSystem {
  distro: LinuxDistro | null;
}

export function useLinux(): LinuxSystem | null {
  const [linux, setLinux] = useState<LinuxSystem | null>(null);
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (active && info.os.toLowerCase().startsWith("linux")) {
            setLinux({ distro: info.distro });
          }
        },
        (error: unknown) => {
          console.error("Reading the app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return linux;
}
