import { useEffect, useState } from "react";
import { getApi, type AudioDevices } from "../../api";

/** The audio devices, kept current while devices are plugged in or removed; `null` while loading. */
export function useAudioDevices(): AudioDevices | null {
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  useEffect(() => {
    let active = true;
    const api = getApi();
    api.audioDevices().then(
      (loaded) => {
        if (active) setDevices(loaded);
      },
      (error: unknown) => {
        console.error("Loading audio devices failed", error);
        if (active) setDevices({ inputs: [], outputs: [] });
      },
    );
    const off = api.on("audioDevicesChanged", ({ devices: changed }) => {
      setDevices(changed);
    });
    return () => {
      active = false;
      off();
    };
  }, []);
  return devices;
}
