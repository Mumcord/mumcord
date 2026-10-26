/**
 * Watches one person's screen share while the component is mounted (FR-2.43, FR-2.44): asks the
 * engine for the stream, draws its frames onto the canvas and stops watching on unmount. Where
 * the webview cannot decode VP9 the engine decodes and sends RGBA pictures instead. Without a
 * first picture within {@link CONNECT_TIMEOUT_MS} (not counting a pause) the attempt ends as
 * unreachable (FR-2.46); `retry` starts a new one.
 */
import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { getApi, toEngineError } from "../../api";
import { screenFrameChannel, webCodecsVp9Supported } from "../../api/screenFrames";
import { createFrameRenderer } from "./frameRenderer";
import { CONNECT_TIMEOUT_MS, watchFailure, type WatchFailure } from "./watchFailure";

export type WatchStatus =
  /** Asked the engine for the stream; no picture yet. */
  | { state: "starting" }
  /** Pictures are arriving. */
  | { state: "live" }
  /** The engine refused or no picture came in time. */
  | { state: "failed"; reason: WatchFailure };

export interface ScreenWatch {
  status: WatchStatus;
  retry: () => void;
}

export function useScreenWatch(
  serverId: number,
  session: number,
  canvasRef: RefObject<HTMLCanvasElement | null>,
  paused: boolean,
): ScreenWatch {
  const [status, setStatus] = useState<WatchStatus>({ state: "starting" });
  const [attempt, setAttempt] = useState(0);
  // Read by the timeout, so pausing does not restart the attempt.
  const pausedRef = useRef(paused);
  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);

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
      const decodeHere = await webCodecsVp9Supported();
      if (isStopped()) return;
      const renderer = createFrameRenderer(canvas, {
        mode: decodeHere ? "vp9" : "rgba",
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
      const channel = screenFrameChannel((frame) => {
        renderer.push(frame);
      });
      const stop = () => {
        renderer.close();
        channel.onmessage = () => undefined;
        api.screenShareUnwatch(serverId, session).catch((error: unknown) => {
          console.warn("stopping a screen share failed", error);
        });
      };
      // Set before the engine answers, so a timeout meanwhile also ends the engine's attempt.
      stopWatching = stop;
      try {
        await api.screenShareWatch(serverId, session, channel);
      } catch (error) {
        // The engine already ended this attempt; only the renderer is left to close.
        stopWatching = null;
        renderer.close();
        fail(error);
        return;
      }
      // Unmounted or timed out while the engine was still starting the stream: `stop` ran.
      if (isStopped()) return;
      if (!decodeHere) await api.screenShareSetDecodeFallback(serverId, session, true).catch(fail);
    };
    void run();

    return () => {
      clearTimeout(timer);
      if (isStopped()) return;
      stopped = true;
      stopWatching?.();
    };
  }, [serverId, session, canvasRef, attempt]);

  const retry = useCallback(() => {
    setAttempt((n) => n + 1);
  }, []);

  return { status, retry };
}
