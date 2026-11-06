/** The cameras of this computer, kept current while the preview is open (FR-8.27). */
import { useEffect, useState } from "react";
import { getApi, type CameraDevice } from "../../../api";

export function useCameraDevices(initial: CameraDevice[]): CameraDevice[] {
  const [devices, setDevices] = useState(initial);
  useEffect(() => {
    const api = getApi();
    const stop = api.on("cameraDevicesChanged", ({ devices: next }) => {
      setDevices(next);
    });
    api.cameraDevicesWatchStart().catch((error: unknown) => {
      console.error("Watching the cameras failed", error);
    });
    return () => {
      stop();
      api.cameraDevicesWatchStop().catch((error: unknown) => {
        console.error("Stopping the camera watch failed", error);
      });
    };
  }, []);
  return devices;
}
