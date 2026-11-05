/** Saves camera choices into the settings without overwriting other changes made meanwhile. */
import { getApi, type Settings } from "../../../api";

export type CameraSettingsPatch = Partial<
  Pick<Settings, "cameraDeviceId" | "cameraDefaultBackground" | "cameraBlurStrength">
>;

export async function saveCameraSettings(patch: CameraSettingsPatch): Promise<void> {
  const api = getApi();
  const current = await api.settingsGet();
  await api.settingsUpdate({ ...current, ...patch });
}
