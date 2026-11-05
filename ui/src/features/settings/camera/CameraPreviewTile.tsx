import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { useLocalPreview } from "./useLocalPreview";

/** The camera errors that have their own text; anything else is "couldn't be started". */
const ERROR_KEYS = new Set([
  "busy",
  "noCamera",
  "permissionDenied",
  "deviceGone",
  "unreadable",
  "cameraFailed",
]);

/**
 * Live self-view of the Settings → Camera page (FR-8.30, mockup 11): the picture with the saved
 * background effect, mirrored when "Mirror my self-view" is on, and the note that it is local.
 */
export function CameraPreviewTile({ mirror }: { mirror: boolean }) {
  const { t } = useTranslation("settings");
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const state = useLocalPreview(canvasRef);
  const failure =
    state.kind === "failed"
      ? t(`camera.errors.${ERROR_KEYS.has(state.reason) ? state.reason : "cameraFailed"}`)
      : null;
  return (
    <div className="settings-camera-preview">
      <div
        className="settings-camera-preview__tile"
        role="img"
        aria-label={failure ?? t("camera.previewLabel")}
      >
        <canvas
          ref={canvasRef}
          className="settings-camera-preview__video"
          data-mirror={mirror ? "" : undefined}
          hidden={state.kind !== "running"}
          aria-hidden="true"
        />
        {failure !== null && <p className="settings-camera-preview__message">{failure}</p>}
        <span className="settings-camera-preview__name" aria-hidden="true">
          {t("camera.previewName")}
        </span>
      </div>
      <span className="settings-hint settings-camera-preview__note">{t("camera.previewNote")}</span>
    </div>
  );
}
