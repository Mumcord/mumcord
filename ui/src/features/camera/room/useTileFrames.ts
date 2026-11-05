/**
 * Draws a camera onto a tile's canvas (FR-8.54): another person's camera through its frame channel
 * (`camera_frames_attach`), the own camera from local frames (`camera_preview_attach`, FR-8.66).
 * Both reuse the screen share renderer: WebCodecs for encoded frames, RGBA pictures drawn directly.
 * While the room is a hidden stage tab the renderer keeps only the latest picture.
 */
import { useEffect, useRef, type RefObject } from "react";
import type { Channel } from "@tauri-apps/api/core";
import { getApi } from "../../../api";
import { screenFrameChannel } from "../../../api/screenFrames";
import { createFrameRenderer, type FrameRenderer } from "../../screenshare/frameRenderer";
import { decodeInEngine, ensureDecodeCodecs } from "./cameraDecode";
import { heightStep } from "./layout";

type Source = { kind: "self" } | { kind: "camera"; serverId: number; sharer: number };

function attach(source: Source, channel: Channel<ArrayBuffer>): Promise<void> {
  const api = getApi();
  if (source.kind === "self") return api.cameraPreviewAttach(channel);
  return ensureDecodeCodecs().then(() =>
    api.cameraFramesAttach(source.serverId, source.sharer, channel),
  );
}

function detach(source: Source, channelId: number): Promise<void> {
  const api = getApi();
  if (source.kind === "self") return api.cameraPreviewDetach(channelId);
  return api.cameraFramesDetach(source.serverId, source.sharer, channelId);
}

/**
 * Runs the frames of `source` into `canvas` while `active`; `hidden` keeps decoding without
 * drawing. `onPicture` reports whether a picture is on the canvas.
 */
export function useTileFrames(
  canvas: RefObject<HTMLCanvasElement | null>,
  source: Source,
  active: boolean,
  hidden: boolean,
  onPicture: (shown: boolean) => void,
): void {
  const rendererRef = useRef<FrameRenderer | null>(null);
  const hiddenRef = useRef(hidden);
  const onPictureRef = useRef(onPicture);
  useEffect(() => {
    onPictureRef.current = onPicture;
  }, [onPicture]);
  const serverId = source.kind === "camera" ? source.serverId : null;
  const sharer = source.kind === "camera" ? source.sharer : null;

  useEffect(() => {
    hiddenRef.current = hidden;
    rendererRef.current?.setHidden(hidden);
  }, [hidden]);

  useEffect(() => {
    const element = canvas.current;
    if (!active || element === null) return;
    const from: Source =
      serverId === null || sharer === null
        ? { kind: "self" }
        : { kind: "camera", serverId, sharer };
    const renderer = createFrameRenderer(element, {
      requestKeyframe: () => {
        if (from.kind === "self") return;
        getApi()
          .cameraRequestKeyframe(from.serverId, from.sharer)
          .catch((error: unknown) => {
            console.warn("camera keyframe request failed", error);
          });
      },
      onFirstPicture: () => {
        onPictureRef.current(true);
      },
      onUnsupported: decodeInEngine,
    });
    renderer.setHidden(hiddenRef.current);
    rendererRef.current = renderer;
    const channel = screenFrameChannel((frame) => {
      renderer.push(frame);
    });
    attach(from, channel).catch((error: unknown) => {
      console.warn("attaching a camera failed", error);
    });
    return () => {
      renderer.close();
      if (rendererRef.current === renderer) rendererRef.current = null;
      channel.onmessage = () => undefined;
      onPictureRef.current(false);
      detach(from, channel.id).catch((error: unknown) => {
        console.warn("detaching a camera failed", error);
      });
    };
  }, [canvas, active, serverId, sharer]);
}

/**
 * Reports the tile's drawn height in device pixels for Auto (FR-8.55), rounded to 180 / 360 / 540 /
 * 720 and only when that step changes; the engine limits how often it is sent on.
 */
export function useTileHeight(
  tile: RefObject<HTMLElement | null>,
  serverId: number,
  sharer: number | null,
): void {
  useEffect(() => {
    const element = tile.current;
    if (element === null || sharer === null || typeof ResizeObserver !== "function") return;
    let sent = 0;
    const observer = new ResizeObserver(() => {
      const step = heightStep(element.getBoundingClientRect().height * window.devicePixelRatio);
      if (step === sent) return;
      sent = step;
      getApi()
        .cameraTileSetHeight(serverId, sharer, step)
        .catch((error: unknown) => {
          console.warn("reporting a camera tile height failed", error);
        });
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [tile, serverId, sharer]);
}
