import { useEffect, useState } from "react";
import { getApi, type AppInfo } from "../../api";

/** Version, OS and packaging of the running app; null while loading or when loading failed. */
export function useAppInfo(): AppInfo | null {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (loaded) => {
          if (active) setInfo(loaded);
        },
        (error: unknown) => {
          console.error("Loading app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return info;
}
