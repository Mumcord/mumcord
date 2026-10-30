import type { AudioDevice } from "../../api";

/** The device shown as selected: the chosen one, else the system default (also when the chosen
 *  one is gone, because the engine then plays on the default). */
export function selectedDeviceId(devices: readonly AudioDevice[], chosen: string | null) {
  const found = devices.find((device) => device.id === chosen);
  return (found ?? devices.find((device) => device.isDefault))?.id ?? null;
}
