/** The live own picture of the preview dialog (FR-8.11): RGBA frames from `camera_preview_attach`. */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { screenFrameChannel } from "../../../api/screenFrames";
import { LockIcon } from "./previewIcons";

export function PreviewPicture({ mirror, live }: { mirror: boolean; live: boolean }) {
  const { t } = useTranslation("camera");
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!live) return;
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    let channel: ReturnType<typeof screenFrameChannel>;
    try {
      channel = screenFrameChannel((frame) => {
        if (frame.kind !== "rgba") return;
        if (canvas.width !== frame.width || canvas.height !== frame.height) {
          canvas.width = frame.width;
          canvas.height = frame.height;
        }
        context.putImageData(new ImageData(frame.data, frame.width, frame.height), 0, 0);
      });
    } catch (error: unknown) {
      // Only outside the desktop app (browser mock without IPC): the dialog works without a picture.
      console.warn("The camera preview picture is not available here", error);
      return;
    }
    const api = getApi();
    api.cameraPreviewAttach(channel).catch((error: unknown) => {
      console.error("Attaching the camera preview failed", error);
    });
    return () => {
      api.cameraPreviewDetach(channel.id).catch((error: unknown) => {
        console.error("Detaching the camera preview failed", error);
      });
    };
  }, [live]);

  return (
    <div className="cam-preview__picture" role="img" aria-label={t("preview.previewLabel")}>
      <canvas
        ref={canvasRef}
        className="cam-preview__canvas"
        data-mirror={mirror ? "" : undefined}
        width={640}
        height={360}
      />
      <span className="cam-preview__tag">
        <LockIcon size={12} /> {t("preview.subtitle")}
      </span>
    </div>
  );
}
