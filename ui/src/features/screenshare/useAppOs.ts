import { useEffect, useState } from "react";
import { getApi } from "../../api";

/** The backend's OS description ("windows x86_64"), empty until the app info arrived. */
export function useAppOs(): string {
  const [os, setOs] = useState("");
  useEffect(() => {
    let active = true;
    getApi()
      .appInfo()
      .then(
        (info) => {
          if (active) setOs(info.os);
        },
        (error: unknown) => {
          console.error("Reading the app info failed", error);
        },
      );
    return () => {
      active = false;
    };
  }, []);
  return os;
}
