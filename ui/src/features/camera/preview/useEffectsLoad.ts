/** The effects state of the running camera, polled about once a second (G-1, FR-8.36). */
import { useEffect, useState } from "react";
import { getApi, type CameraEffects } from "../../../api";

const POLL_MS = 1000;

export function useEffectsLoad(): CameraEffects | null {
  const [effects, setEffects] = useState<CameraEffects | null>(null);
  useEffect(() => {
    let active = true;
    let failedOnce = false;
    const poll = () => {
      getApi()
        .cameraEffects()
        .then(
          (next) => {
            if (active) setEffects(next);
          },
          (error: unknown) => {
            // Logged once: the poll would otherwise repeat the same error every second.
            if (!failedOnce) console.error("Reading the camera effects failed", error);
            failedOnce = true;
          },
        );
    };
    poll();
    const timer = window.setInterval(poll, POLL_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);
  return effects;
}
