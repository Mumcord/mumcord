import type { CameraDevice, CameraReduceMotion } from "../../../api";

/** The device the select shows: the saved one if it is still there, else the system default. */
export function shownDeviceId(devices: readonly CameraDevice[], saved: string | null): string {
  if (saved !== null && devices.some((device) => device.id === saved)) return saved;
  return (devices.find((device) => device.isDefault) ?? devices[0])?.id ?? "";
}

/** Whether "Reduce motion" is on: the saved choice, or the system setting while it follows it. */
export function reduceMotionOn(setting: CameraReduceMotion, systemReduce: boolean): boolean {
  return setting === "system" ? systemReduce : setting === "on";
}
