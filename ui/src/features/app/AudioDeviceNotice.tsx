import { SettingsNotice, type SettingsNoticeApi } from "./SettingsNotice";

/**
 * What the notice needs from the engine. `pending` is true once when a device choice saved from
 * the old device list could not be matched and was reset to the system default (issue #183);
 * `dismiss` clears it.
 */
export type AudioDeviceNoticeApi = SettingsNoticeApi;

/** One-time notice that the saved audio device was reset; "Open audio settings" leads there. */
export function AudioDeviceNotice({
  api,
  onOpenSettings,
}: {
  api?: AudioDeviceNoticeApi;
  onOpenSettings: () => void;
}) {
  return <SettingsNotice api={api} texts="audioDevice" onOpenSettings={onOpenSettings} />;
}
