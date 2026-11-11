/**
 * The room bar (FR-8.68, mockups 05–10): Camera on (turns it off) or Turn on camera, Background
 * (only while on air), Hide self-view, Pop out, full screen and Close room. In the pop-out window
 * Pop out becomes "Always on top" and "Pop back in", and the room cannot be closed from there.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { popIn, setPopoutOnTop, useInPopout, usePopouts } from "../../media/popoutStore";
import { CameraStoppedNotice } from "./CameraStoppedNotice";
import { RoomIcon } from "./icons";
import { OwnViewers } from "./OwnViewers";
import { setSelfHidden } from "./roomChoices";
import { useRoomBarOnScreen } from "./useCameraRoomShown";

export interface RoomBarProps {
  serverId: number;
  onAir: boolean;
  selfHidden: boolean;
  /** Opens the preview dialog (FR-8.68 "Turn on camera"); absent where it cannot open. */
  onTurnOn: (() => void) | null;
  /** The "Background: {name}" button with its menu (FR-8.40), shown while on air. */
  background: ReactNode;
  onPopOut: () => void;
  fullscreen: boolean;
  onFullscreen: () => void;
  onClose: () => void;
}

export function RoomBar({
  serverId,
  onAir,
  selfHidden,
  onTurnOn,
  background,
  onPopOut,
  fullscreen,
  onFullscreen,
  onClose,
}: RoomBarProps) {
  const { t } = useTranslation("camera");
  const { t: tc } = useTranslation("common");
  const inPopout = useInPopout();
  const onTop = usePopouts().camera.onTop;
  // A room that is open behind another item leaves the notice to the mini bar's place.
  const barOnScreen = useRoomBarOnScreen(serverId);

  const turnOff = () => {
    getApi()
      .cameraTurnOff(serverId)
      .catch((error: unknown) => {
        console.error("turning the camera off failed", error);
      });
  };

  return (
    <div className="cam-room__bar" role="toolbar" aria-label={t("room.controls")}>
      {(inPopout || barOnScreen) && <CameraStoppedNotice serverId={serverId} />}
      {onAir ? (
        <button
          type="button"
          className="cam-btn cam-btn--onair"
          aria-pressed="true"
          aria-label={t("turnOffLabel")}
          onClick={turnOff}
        >
          <span className="cam-dot" aria-hidden="true" />
          {t("cameraOn")}
        </button>
      ) : (
        onTurnOn !== null && (
          <button type="button" className="cam-btn" onClick={onTurnOn}>
            <RoomIcon name="camera" />
            {t("turnOn")}
          </button>
        )
      )}
      {onAir && background}
      {onAir && <OwnViewers serverId={serverId} />}
      {onAir && (
        <button
          type="button"
          className="cam-btn"
          aria-pressed={selfHidden}
          onClick={() => {
            setSelfHidden(!selfHidden);
          }}
        >
          <RoomIcon name="eyeOff" />
          {t("room.hideSelf")}
        </button>
      )}
      {inPopout ? (
        <>
          <button
            type="button"
            className="cam-btn"
            aria-pressed={onTop}
            onClick={() => {
              setPopoutOnTop("camera", !onTop, tc("media.onTopFailed"));
            }}
          >
            {tc("media.onTop")}
          </button>
          <button
            type="button"
            className="cam-btn"
            aria-keyshortcuts="Control+W"
            onClick={() => {
              popIn("camera", tc("media.popInFailed"));
            }}
          >
            {tc("media.popIn")}
          </button>
        </>
      ) : (
        <button
          type="button"
          className="cam-btn"
          aria-label={t("room.popOutLabel")}
          onClick={onPopOut}
        >
          <RoomIcon name="popOut" />
          {t("room.popOut")}
        </button>
      )}
      <button
        type="button"
        className="cam-btn cam-btn--icon"
        aria-label={fullscreen ? tc("media.exitFullscreen") : tc("media.fullscreen")}
        data-tooltip={fullscreen ? tc("media.exitFullscreen") : tc("media.fullscreen")}
        aria-pressed={fullscreen}
        onClick={onFullscreen}
      >
        <RoomIcon name={fullscreen ? "exitFullscreen" : "fullscreen"} />
      </button>
      {!inPopout && (
        <button type="button" className="cam-btn cam-btn--leave" onClick={onClose}>
          <RoomIcon name="leave" />
          {t("room.close")}
        </button>
      )}
    </div>
  );
}
