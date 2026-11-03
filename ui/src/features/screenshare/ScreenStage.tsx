/**
 * The picture of a watched share with its status line: connecting, paused or why watching
 * failed (FR-2.43 "This share is full", FR-2.46 direct connection needed) with "Try again"
 * and "Stop watching".
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { p2pBlockText } from "../p2p/relayReason";
import { MockPicture } from "../media/MockPicture";
import { mockPlaceholder } from "../media/mockPlaceholder";
import { useInPopout } from "../media/popoutStore";
import { useScreenShareOf } from "./sharesStore";
import { useScreenWatch, type ScreenWatch } from "./useScreenWatch";
import type { StreamMeter } from "./streamStats";
import { stopWatching } from "./watching";

export interface ScreenStageProps {
  serverId: number;
  session: number;
  name: string;
  paused: boolean;
  /** Counts the frames for the chips and the stream details (issue #163). */
  meter?: StreamMeter;
  /** Hands the codec and the frame-rate cap to the header, which owns the quality menu. */
  onWatch?: (watch: Pick<ScreenWatch, "codec" | "maxFps" | "setMaxFps">) => void;
}

export function ScreenStage({ serverId, session, name, paused, meter, onWatch }: ScreenStageProps) {
  const { t } = useTranslation("screenviewer");
  const { t: tP2p } = useTranslation("p2p");
  const relayReason = useScreenShareOf(serverId, session)?.relayReason ?? null;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { status, retry, codec, maxFps, setMaxFps } = useScreenWatch(
    serverId,
    session,
    canvasRef,
    paused,
    meter,
  );
  useEffect(() => {
    onWatch?.({ codec, maxFps, setMaxFps });
  }, [onWatch, codec, maxFps, setMaxFps]);
  const inPopout = useInPopout();

  let message: string | null = null;
  if (status.state === "failed")
    message =
      status.reason === "p2pOff" || status.reason === "relayUnavailable"
        ? p2pBlockText(tP2p, status.reason, relayReason)
        : t(status.reason, { name });
  else if (paused) message = t("paused", { name });
  else if (status.state === "starting") message = t("starting", { name });

  return (
    <div className="screen-stage">
      <div
        className="screen-stage__picture"
        role="img"
        aria-label={t("picture", { name })}
        hidden={status.state === "failed" && mockPlaceholder() === null}
      >
        {mockPlaceholder() === null ? (
          <canvas ref={canvasRef} className="screen-stage__canvas" />
        ) : (
          <MockPicture label={t("picture", { name })} />
        )}
      </div>
      <div className="screen-stage__status" role="status">
        {message !== null && <p className="screen-stage__message">{message}</p>}
        {status.state === "failed" && (
          <div className="screen-stage__actions">
            <button type="button" className="button" onClick={retry}>
              {t("retry")}
            </button>
            {!inPopout && (
              <button type="button" className="button" onClick={stopWatching}>
                {t("stop")}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
