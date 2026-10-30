import { SettingsNotice, type SettingsNoticeApi } from "./SettingsNotice";

/**
 * What the notice needs from the engine. `pending` is true once for users who had the window
 * overlay switched on before the update; `dismiss` stores `window_overlay_notice_shown`.
 */
export type WindowOverlayNoticeApi = SettingsNoticeApi;

/**
 * One-time notice that the window overlay is gone (FR-2.33, Q2.9). "Open overlay settings" also
 * leads to the in-game overlay.
 */
export function WindowOverlayNotice({
  api,
  onOpenSettings,
}: {
  api?: WindowOverlayNoticeApi;
  onOpenSettings: () => void;
}) {
  return <SettingsNotice api={api} texts="windowOverlay" onOpenSettings={onOpenSettings} />;
}
