/**
 * Watches one person's screen share while the component is mounted (FR-2.43, FR-2.44): asks the
 * engine for the stream, draws its frames onto the canvas and stops watching on unmount. For each
 * codec the webview's WebCodecs cannot decode (AV1, VP9) the engine decodes and sends RGBA
 * pictures instead (FR-7.59). It also tells the codec on screen and carries the viewer's
 * frame-rate choice to the engine (FR-7.51), which drops temporal layers to meet it. Without a
 * first picture within {@link CONNECT_TIMEOUT_MS} (not counting a pause) the attempt ends as
 * unreachable (FR-2.46); `retry` starts a new one.
 *
 * The main window owns the engine watch. While the share is popped out (issue #164) the pop-out
 * only attaches its own frame channel to that watch, and the main window keeps the watch open and
 * attaches again when the view docks.
 */
import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { getApi, toEngineError, type VideoCodec } from "../../api";
import { screenFrameChannel, VIDEO_CODECS, webCodecsCodecs } from "../../api/screenFrames";
import { isPoppedOut, popouts, useInPopout } from "../media/popoutStore";
import { keepWatch, takeKeptWatch } from "./frameHandover";
import { createFrameRenderer, type FrameRenderer } from "./frameRenderer";
import type { StreamMeter } from "./streamStats";
import { CONNECT_TIMEOUT_MS, watchFailure, type WatchFailure } from "./watchFailure";

export type WatchStatus =
  /** Asked the engine for the stream; no picture yet. */
  | { state: "starting" }
  /** Pictures are arriving. */
  | { state: "live" }
  /** The engine refused or no picture came in time. */
  | { state: "failed"; reason: WatchFailure };

/** The viewer's frame-rate choice (FR-7.51): Auto (`null`, the sharer's rate) or a cap. */
export type ViewerFps = null | 30 | 15;

export interface ScreenWatch {
  status: WatchStatus;
  retry: () => void;
  /** Codec of the frames on screen; `null` until the first frame. */
  codec: VideoCodec | null;
  maxFps: ViewerFps;
  setMaxFps: (fps: ViewerFps) => void;
}

export function useScreenWatch(
  serverId: number,
  session: number,
  canvasRef: RefObject<HTMLCanvasElement | null>,
  paused: boolean,
  meter?: StreamMeter,
  /** Another stage tab is the main item: keep watching, but skip drawing (FR-9.44). */
  hidden = false,
): ScreenWatch {
  const [status, setStatus] = useState<WatchStatus>({ state: "starting" });
  const [attempt, setAttempt] = useState(0);
  const [codec, setCodec] = useState<VideoCodec | null>(null);
  const [maxFps, setMaxFpsState] = useState<ViewerFps>(null);
  // Read when a watch starts, so changing it does not restart the attempt.
  const maxFpsRef = useRef<ViewerFps>(null);
  // Set once the engine has the stream, for frame-rate changes during the watch.
  const liveRef = useRef(false);
  const inPopout = useInPopout();
  // Read by the timeout, so pausing does not restart the attempt.
  const pausedRef = useRef(paused);
  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);
  const hiddenRef = useRef(hidden);
  const rendererRef = useRef<FrameRenderer | null>(null);
  useEffect(() => {
    hiddenRef.current = hidden;
    rendererRef.current?.setHidden(hidden);
  }, [hidden]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (canvas === null) return;
    const api = getApi();
    let stopped = false;
    // A function call, so the checks after each await see an unmount in between.
    const isStopped = () => stopped;
    let stopWatching: (() => void) | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setStatus({ state: "starting" });

    const end = (reason: WatchFailure) => {
      if (isStopped()) return;
      stopped = true;
      clearTimeout(timer);
      stopWatching?.();
      stopWatching = null;
      setStatus({ state: "failed", reason });
    };
    const fail = (error: unknown) => {
      const engineError = toEngineError(error);
      console.warn("watching a screen share failed", engineError.code, engineError.message);
      end(watchFailure(engineError));
    };
    const armTimeout = () => {
      timer = setTimeout(() => {
        if (pausedRef.current) armTimeout();
        else end("unreachable");
      }, CONNECT_TIMEOUT_MS);
    };
    armTimeout();

    const run = async () => {
      const inWebview = await webCodecsCodecs();
      if (isStopped()) return;
      // Codecs the engine decodes for this webview; grows when WebCodecs fails on one.
      const inEngine = new Set(VIDEO_CODECS.filter((c) => !inWebview.includes(c)));
      let engineHasStream = false;
      const sendDecodeCodecs = () =>
        api.screenShareSetDecodeCodecs(serverId, session, [...inEngine]).catch(fail);
      const renderer = createFrameRenderer(canvas, {
        onFrame: (frame, atMs) => {
          setCodec(frame.codec);
          // Decoded pictures still tell the size and the frame rate, so the chips work there too.
          const encoded = frame.kind === "encoded";
          meter?.frame({
            atMs,
            timestampUs: encoded ? frame.timestampUs : null,
            bytes: encoded ? frame.data.byteLength : null,
            width: frame.width,
            height: frame.height,
          });
        },
        onDrop: (atMs) => {
          meter?.drop(atMs);
        },
        onUnsupported: (failed) => {
          if (inEngine.has(failed)) return;
          console.warn("WebCodecs cannot play this share; the engine decodes it", failed);
          inEngine.add(failed);
          if (engineHasStream) void sendDecodeCodecs();
        },
        requestKeyframe: () => {
          api.screenShareRequestKeyframe(serverId, session).catch((error: unknown) => {
            console.warn("screen share keyframe request failed", error);
          });
        },
        onFirstPicture: () => {
          if (isStopped()) return;
          clearTimeout(timer);
          setStatus({ state: "live" });
        },
      });
      renderer.setHidden(hiddenRef.current);
      rendererRef.current = renderer;
      const channel = screenFrameChannel((frame) => {
        renderer.push(frame);
      });
      const stop = () => {
        renderer.close();
        if (rendererRef.current === renderer) rendererRef.current = null;
        channel.onmessage = () => undefined;
        if (inPopout) return;
        if (isPoppedOut(popouts(), "share", serverId, session)) {
          keepWatch({ serverId, session });
          return;
        }
        api.screenShareUnwatch(serverId, session).catch((error: unknown) => {
          console.warn("stopping a screen share failed", error);
        });
      };
      // Set before the engine answers, so a timeout meanwhile also ends the engine's attempt.
      stopWatching = stop;
      const attach = inPopout || takeKeptWatch(serverId, session);
      try {
        if (attach) await api.screenShareFramesAttach(serverId, session, channel);
        else await api.screenShareWatch(serverId, session, channel);
      } catch (error) {
        // The engine already ended this attempt; only the renderer is left to close.
        stopWatching = null;
        renderer.close();
        fail(error);
        return;
      }
      // Unmounted or timed out while the engine was still starting the stream: `stop` ran.
      if (isStopped()) return;
      engineHasStream = true;
      liveRef.current = true;
      // Always sent: a watch kept from another window may carry that window's choices.
      await sendDecodeCodecs();
      if (isStopped()) return;
      await api.screenShareSetViewerFps(serverId, session, maxFpsRef.current).catch(fail);
    };
    void run();

    return () => {
      clearTimeout(timer);
      liveRef.current = false;
      if (isStopped()) return;
      stopped = true;
      stopWatching?.();
    };
  }, [serverId, session, canvasRef, attempt, meter, inPopout]);

  const retry = useCallback(() => {
    setAttempt((n) => n + 1);
  }, []);

  const setMaxFps = useCallback(
    (fps: ViewerFps) => {
      maxFpsRef.current = fps;
      setMaxFpsState(fps);
      if (!liveRef.current) return;
      getApi()
        .screenShareSetViewerFps(serverId, session, fps)
        .catch((error: unknown) => {
          console.warn("setting the screen share frame rate failed", error);
        });
    },
    [serverId, session],
  );

  return { status, retry, codec, maxFps, setMaxFps };
}
