/**
 * Start and stop of the own screen share (FR-2.42), used by the share button and the Apps menu.
 * Starting opens the share start dialog first (FR-7.35); its picks go to the engine with the
 * start, before the system's picker opens.
 */
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, toEngineError } from "../../api";
import { shareBlockText } from "./availability";
import type { ShareStartDialogProps } from "./start/ShareStartDialog";
import type { StartPicks } from "./start/startPicks";
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

export function useShareActions({ serverId, own, block, platform, recheck }: OwnShareState) {
  const { t } = useTranslation("screenshare");
  const { t: tP2p } = useTranslation("p2p");
  const [starting, setStarting] = useState(false);
  const [failed, setFailed] = useState<ShareFailure | null>(null);
  const [asking, setAsking] = useState(false);

  const disabled = block !== null || starting || asking || serverId === null;

  /** Opens the start dialog; the share starts on its confirm. */
  const start = async () => {
    if (disabled) return;
    setFailed(null);
    const now = await recheck();
    if (now !== null) {
      announce(shareBlockText(t, tP2p, now), "assertive");
      return;
    }
    setAsking(true);
  };

  const confirm = async (picks: StartPicks) => {
    setAsking(false);
    if (serverId === null) return;
    setStarting(true);
    try {
      // "Change window": the system picker only opens with a fresh share, so the old one ends.
      if (own !== null) await getApi().screenShareStop(serverId);
      // Resolves without a share when the picker was closed; nothing changes then.
      await getApi().screenShareStart(
        serverId,
        (platform?.sound ?? false) && picks.sound,
        picks.choice,
        picks.codec,
      );
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

  const startDialog: ShareStartDialogProps | null =
    asking && serverId !== null
      ? {
          serverId,
          soundAvailable: platform?.sound ?? false,
          onCancel: () => {
            setAsking(false);
          },
          onConfirm: (picks) => void confirm(picks),
        }
      : null;

  return { starting, failed, disabled, start, stop, startDialog };
}
