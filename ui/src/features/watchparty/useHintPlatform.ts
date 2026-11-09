/**
 * Which package manager the player hint has to speak, from the app info (FR-2.41a, FR-13.34).
 * Linux names the command of the distribution's package manager; FreeBSD always needs the
 * GStreamer packages it names. Every other platform decodes with what the system ships and gets
 * no hint, so the answer stays `null` there and until the app info arrives.
 */
import { useEffect, useState } from "react";
import { getApi, type LinuxDistro } from "../../api";

export type HintPlatform = { kind: "linux"; distro: LinuxDistro | null } | { kind: "freebsd" };

export function hintPlatformOf(os: string): HintPlatform | null {
  const name = os.toLowerCase();
  if (name.startsWith("linux")) return { kind: "linux", distro: null };
  if (name.startsWith("freebsd")) return { kind: "freebsd" };
  return null;
}

export function useHintPlatform(): HintPlatform | null {
  const [platform, setPlatform] = useState<HintPlatform | null>(null);
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (!active) return;
          const known = hintPlatformOf(info.os);
          if (known === null) return;
          setPlatform(
            known.kind === "linux" ? { kind: "linux", distro: info.distro } : known,
          );
        },
        (error: unknown) => {
          console.error("Reading the app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return platform;
}
