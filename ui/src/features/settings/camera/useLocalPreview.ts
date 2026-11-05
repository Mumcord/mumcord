import { useEffect, useState, type RefObject } from "react";
import { getApi, toEngineError } from "../../../api";
import { screenFrameChannel, type ScreenFrame } from "../../../api/screenFrames";

/** What the Settings preview shows: starting, the picture, or why there is none. */
export type LocalPreviewState =
  { kind: "starting" } | { kind: "running" } | { kind: "failed"; reason: string };

/**
 * Runs the Settings → Camera preview while the page is open (FR-8.30): starts the local camera,
 * draws its pictures (RGBA, at most 640×360) on `canvas` and stops it when the page goes away,
 * so the camera light never stays on behind a closed Settings window.
 */
export function useLocalPreview(canvasRef: RefObject<HTMLCanvasElement | null>): LocalPreviewState {
  const [state, setState] = useState<LocalPreviewState>({ kind: "starting" });

  useEffect(() => {
    const api = getApi();
    let stopped = false;
    // A function, so checks after an `await` see the cleanup's change.
    const isStopped = () => stopped;
    const draw = (frame: ScreenFrame) => {
      const target = canvasRef.current;
      if (isStopped() || target === null || frame.kind !== "rgba") return;
      if (target.width !== frame.width) target.width = frame.width;
      if (target.height !== frame.height) target.height = frame.height;
      target
        .getContext("2d")
        ?.putImageData(new ImageData(frame.data, frame.width, frame.height), 0, 0);
    };
    // Channels need the Tauri runtime; the browser mock has no camera picture to send.
    const channel = api.kind === "tauri" ? screenFrameChannel(draw) : null;

    void (async () => {
      try {
        await api.cameraLocalPreviewStart();
        if (isStopped()) return;
        if (channel !== null) await api.cameraPreviewAttach(channel);
        if (!isStopped()) setState({ kind: "running" });
      } catch (error) {
        const engineError = toEngineError(error);
        if (engineError.code !== "camera") console.error("Settings camera preview failed", error);
        if (!isStopped()) setState({ kind: "failed", reason: engineError.message });
      }
    })();

    return () => {
      stopped = true;
      if (channel !== null) {
        api.cameraPreviewDetach(channel.id).catch((error: unknown) => {
          console.error("Detaching the Settings camera preview failed", error);
        });
      }
      api.cameraLocalPreviewStop().catch((error: unknown) => {
        console.error("Stopping the Settings camera preview failed", error);
      });
    };
  }, [canvasRef]);

  return state;
}
