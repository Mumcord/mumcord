/**
 * Shows the camera preview dialog while `openCameraPreview` asked for it. Cancel and Escape close
 * it through `closeCameraPreview` first; the dialog's session turns the camera off as it unmounts,
 * so a late `cameraChanged` from the preview can't reopen it.
 */
import { useEffect } from "react";
import { CameraPreviewDialog } from "./CameraPreviewDialog";
import { closeCameraPreview, useCameraPreviewTarget } from "../previewStore";

export function CameraPreviewHost() {
  const target = useCameraPreviewTarget();
  const opener = target?.opener ?? null;
  useEffect(
    () => () => {
      if (opener?.isConnected) opener.focus();
    },
    [opener],
  );
  if (target === null) return null;
  return (
    <CameraPreviewDialog
      key={target.serverId}
      serverId={target.serverId}
      onClose={closeCameraPreview}
    />
  );
}
