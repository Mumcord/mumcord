/**
 * "Camera stopped: …" (spec 08 §5.6, FR-8.23, FR-8.24): the own camera stopped by itself while
 * it was on. Shown in the room bar and below the camera mini bar until the user dismisses it or
 * turns the camera on again.
 */
import { useTranslation } from "react-i18next";
import { MediaIcon } from "../../media/icons";
import { stoppedText } from "../preview/previewLogic";
import { dismissCameraStopped, useCameraStopped } from "./roomStore";
import "./CameraStoppedNotice.css";

export function CameraStoppedNotice({ serverId }: { serverId: number }) {
  const { t } = useTranslation("camera");
  const failure = useCameraStopped(serverId);
  if (failure === null) return null;
  return (
    <div className="cam-stopped" role="alert">
      <span className="cam-stopped__text">{stoppedText(t, failure)}</span>
      <button
        type="button"
        className="button cam-stopped__dismiss"
        aria-label={t("errors.dismissStopped")}
        data-tooltip={t("errors.dismissStopped")}
        onClick={dismissCameraStopped}
      >
        <MediaIcon name="close" size={12} />
      </button>
    </div>
  );
}
