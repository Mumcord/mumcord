/**
 * Camera button in the user panel next to mute and deafen (FR-8.9, mockups 01, 02, 05). Off, it
 * opens the preview; on air, it is red and turns the camera off at once (FR-8.17, FR-8.19).
 * Blocked, it stays focusable and its tooltip names the reason (FR-8.4, FR-8.16).
 */
import { useTranslation } from "react-i18next";
import { CameraIcon } from "./CameraIcon";
import { useCameraEntry, useOnAirAnnouncements } from "./useCameraEntry";
import "./cameraEntry.css";

export function CameraButton({ serverId }: { serverId: number | null }) {
  const { t } = useTranslation("camera");
  const { onAir, blockText, toggle } = useCameraEntry(serverId);
  useOnAirAnnouncements();
  return (
    <span className="userpanel__split">
      <button
        type="button"
        className="userpanel__toggle camera-button"
        data-onair={onAir ? "" : undefined}
        aria-pressed={onAir}
        aria-disabled={blockText !== null || undefined}
        aria-label={onAir ? t("turnOffLabel") : t("turnOn")}
        data-tooltip={onAir ? t("turnOff") : (blockText ?? t("turnOn"))}
        onClick={toggle}
      >
        <span className="userpanel__icon">
          <CameraIcon size={18} off={!onAir} />
        </span>
      </button>
    </span>
  );
}

/** "● Camera on", shown under the own name instead of the status while on air (FR-8.17). */
export function CameraOnLine() {
  const { t } = useTranslation("camera");
  return (
    <span className="camera-on-line">
      <span aria-hidden="true">● </span>
      {t("cameraOn")}
    </span>
  );
}
