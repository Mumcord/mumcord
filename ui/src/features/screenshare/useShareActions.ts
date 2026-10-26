/** Start and stop of the own screen share (FR-2.42), used by the share button and the Apps menu. */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, toEngineError } from "../../api";
import type { OwnShareState } from "./useOwnShare";

export type ShareFailure =
  | "startFailed"
  | "stopFailed"
  /** macOS does not allow Mumcord to record the screen; the user has to allow it. */
  | "screenRecordingDenied";

/** Why starting failed, as the sharer is told. */
export function startFailure(error: unknown): ShareFailure {
  const engineError = toEngineError(error);
  return engineError.code === "permissionDenied" && engineError.message === "screenRecording"
    ? "screenRecordingDenied"
    : "startFailed";
}

export function useShareActions({ serverId, block, platform, recheck }: OwnShareState) {
  const { t } = useTranslation("screenshare");
  const [starting, setStarting] = useState(false);
  const [failed, setFailed] = useState<ShareFailure | null>(null);

  const disabled = block !== null || starting || serverId === null;

  const start = async () => {
    if (disabled) return;
    setFailed(null);
    const now = await recheck();
    if (now !== null) {
      announce(t(`blocked.${now}`), "assertive");
      return;
    }
    setStarting(true);
    try {
      // Resolves without a share when the picker was closed; nothing changes then.
      await getApi().screenShareStart(serverId, platform?.sound ?? false);
    } catch (error: unknown) {
      console.error("Starting the screen share failed", error);
      setFailed(startFailure(error));
    } finally {
      setStarting(false);
    }
  };

  const stop = () => {
    if (serverId === null) return;
    setFailed(null);
    getApi()
      .screenShareStop(serverId)
      .then(
        () => {
          announce(t("stopped"));
        },
        (error: unknown) => {
          console.error("Stopping the screen share failed", error);
          setFailed("stopFailed");
        },
      );
  };

  return { starting, failed, disabled, start, stop };
}
