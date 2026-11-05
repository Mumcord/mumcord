/** Shows the camera preview dialog while `openCameraPreview` asked for it. */
import { useEffect } from "react";
import { CameraPreviewDialog } from "./CameraPreviewDialog";
import { closeCameraPreview, useCameraPreviewTarget } from "./previewStore";

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
