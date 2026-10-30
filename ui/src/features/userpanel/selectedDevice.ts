import type { AudioDevice } from "../../api";

/** The id of the "System default" row, which stores "follow the system default" (`null`). */
export const SYSTEM_DEFAULT = "";

/** The row shown as selected: the chosen device, else "System default" (also when the chosen
 *  device is gone, because the engine then plays on the default). */
export function selectedDeviceId(devices: readonly AudioDevice[], chosen: string | null): string {
  return chosen !== null && devices.some((device) => device.id === chosen)
    ? chosen
    : SYSTEM_DEFAULT;
}
