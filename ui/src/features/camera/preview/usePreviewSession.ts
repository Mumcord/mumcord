/**
 * The camera behind the preview dialog: starts the local preview (nothing is sent, FR-8.10),
 * turns the camera on (FR-8.13) and off again on Cancel / Escape (FR-8.12).
 */
import { useEffect, useRef, useState } from "react";
import { getApi, type CameraFailure, type CameraState } from "../../../api";
import { cameraFailureOf } from "./previewLogic";

export interface PreviewSession {
  /** `off` until the preview runs; `requesting` while Turn on camera waits for the broadcast. */
  state: CameraState;
  failure: CameraFailure | null;
  turnOn: () => void;
  /** Turns the camera off and closes the dialog. */
  cancel: () => void;
  /** Asks for the camera again after a refused access or a failed start. */
  retry: () => void;
}

export function usePreviewSession(serverId: number, onClose: () => void): PreviewSession {
  const [state, setState] = useState<CameraState>("off");
  const [failure, setFailure] = useState<CameraFailure | null>(null);
  // Set once the camera is on air: closing the dialog then must not turn it off.
  const onAir = useRef(false);
  const closed = useRef(false);
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);

  const start = () => {
    getApi()
      .cameraPreviewStart(serverId)
      .then(
        () => {
          if (!closed.current) setState((now) => (now === "off" ? "preview" : now));
        },
        (error: unknown) => {
          if (!closed.current) setFailure(cameraFailureOf(error));
        },
      );
  };

  useEffect(() => {
    closed.current = false;
    const api = getApi();
    const stop = api.on("cameraChanged", ({ view }) => {
      if (view.serverId !== serverId) return;
      setState(view.state);
      if (view.error !== null) setFailure(view.error);
      if (view.state === "onAir") {
        onAir.current = true;
        close.current();
      }
    });
    start();
    return () => {
      stop();
      closed.current = true;
      if (!onAir.current) {
        api.cameraTurnOff(serverId).catch((error: unknown) => {
          console.error("Turning the camera off failed", error);
        });
      }
    };
    // Starts once per dialog; `start` only reads the server id.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId]);

  const turnOn = () => {
    if (state !== "preview") return;
    setFailure(null);
    setState("requesting");
    getApi()
      .cameraTurnOn(serverId)
      .then(
        () => {
          onAir.current = true;
          close.current();
        },
        (error: unknown) => {
          if (closed.current) return;
          setFailure(cameraFailureOf(error));
          // A failed request leaves the camera in the preview (G-1), ready for a retry.
          setState((now) => (now === "requesting" ? "preview" : now));
        },
      );
  };

  const retry = () => {
    setFailure(null);
    start();
  };

  return { state, failure, turnOn, cancel: onClose, retry };
}
