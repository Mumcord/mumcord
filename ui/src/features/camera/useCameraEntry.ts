/**
 * State and action shared by the camera entry points (user-panel button, Apps menu): turn the
 * camera off while it is on air (FR-8.19), else open the preview (FR-8.10) unless blocked.
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi } from "../../api";
import { useVoiceServerId } from "../channels";
import {
  cameraBlock,
  cameraBlockKey,
  useCameraUnavailable,
  useOwnCamera,
  type CameraBlock,
} from "./availability";
import { openCameraPreview } from "./previewStore";

export interface CameraEntry {
  onAir: boolean;
  /** Why it can't be turned on; always `null` while on air (turning off always works). */
  block: CameraBlock | null;
  /** The block's text, or `null`. */
  blockText: string | null;
  /** Turns the camera off when on air, else opens the preview; does nothing while blocked. */
  toggle: () => void;
}

export function useCameraEntry(serverId: number | null): CameraEntry {
  const { t } = useTranslation("camera");
  const own = useOwnCamera();
  const voiceServerId = useVoiceServerId();
  const reason = useCameraUnavailable(serverId);
  const onAir = own?.state === "onAir";
  const block = onAir ? null : cameraBlock(serverId, voiceServerId, reason);
  const toggle = () => {
    if (onAir) {
      getApi()
        .cameraTurnOff(own.serverId)
        .catch((error: unknown) => {
          console.error("Turning the camera off failed", error);
        });
    } else if (block === null && serverId !== null) {
      openCameraPreview(serverId);
    }
  };
  return { onAir, block, blockText: block === null ? null : t(cameraBlockKey(block)), toggle };
}

/** Announces "Your camera is on" / "Your camera is off" on every change (FR-8.17). Mount once. */
export function useOnAirAnnouncements(): void {
  const { t } = useTranslation("camera");
  const onAir = useOwnCamera()?.state === "onAir";
  const previous = useRef(onAir);
  useEffect(() => {
    if (previous.current === onAir) return;
    previous.current = onAir;
    announce(onAir ? t("cameraOnLabel") : t("cameraOffLabel"));
  }, [onAir, t]);
}
