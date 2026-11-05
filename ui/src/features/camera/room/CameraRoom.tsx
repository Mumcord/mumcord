/**
 * The camera room (spec 08 §4.8, mockups 05–07): a dark media card with the header, the tiles in
 * the Grid or Speaker layout, the people without a camera and the room bar. It renders in the
 * media area of the main window and in the camera pop-out; it keeps its dark picture background
 * in both themes (FR-8.70).
 */
import { useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useAvatars } from "../../members/useAvatars";
import { closeMedia } from "../../media/mediaView";
import { popOut, usePopoutTitle } from "../../media/popoutStore";
import { useStageShown } from "../../media/stageContext";
import { useStageLabel } from "../../media/stageRegistry";
import { useFullscreen } from "../../media/useFullscreen";
import { CameraTile } from "./CameraTile";
import { RoomBar } from "./RoomBar";
import { RoomHeader } from "./RoomHeader";
import { forgetTileChoices } from "./roomChoices";
import { RoomStage } from "./RoomStage";
import type { TileModel } from "./useRoomModel";
import { useRoomModel } from "./useRoomModel";
import { useCameraAnnouncements } from "./useCameraAnnouncements";
import { useCameraLook } from "./useCameraLook";
import { WithoutCamera } from "./WithoutCamera";
import "./camera.css";

export interface CameraRoomProps {
  serverId: number;
  /** Opens the preview dialog; `null` where it cannot open (the pop-out). */
  onTurnOn: (() => void) | null;
  /** The "Background: {name}" control of the room bar (FR-8.40). */
  background?: ReactNode;
  /** The tile menu button of a tile (spec 08 §5.3); the room shows none without it. */
  renderTileMenu?: (tile: TileModel) => ReactNode;
}

/** Closes the room: the media area lets it go and the per-tile choices are forgotten. */
function closeCameraRoom(): void {
  forgetTileChoices();
  closeMedia("camera");
}

export function CameraRoom({ serverId, onTurnOn, background, renderTileMenu }: CameraRoomProps) {
  const { t } = useTranslation("camera");
  const { t: tc } = useTranslation("common");
  const model = useRoomModel(serverId);
  const shown = useStageShown();
  const { mirror, reduceMotion } = useCameraLook();
  const ref = useRef<HTMLElement>(null);
  const { fullscreen, toggle } = useFullscreen(ref);
  const avatarOf = useAvatars(
    [
      ...model.tiles.flatMap((tile) => (tile.user === undefined ? [] : [tile.user])),
      ...model.withoutCamera,
    ],
    serverId,
  );
  const title = usePopoutTitle(serverId, t("room.title"));
  useStageLabel("camera", t("room.title"));
  useCameraAnnouncements(model.tiles);

  return (
    <section
      ref={ref}
      className="cam-room"
      aria-label={t("room.label", { count: model.cameraCount })}
      data-fullscreen={fullscreen || undefined}
      data-reduce-motion={reduceMotion || undefined}
    >
      <RoomHeader model={model} />
      <RoomStage
        model={model}
        renderTile={(tile) => (
          <CameraTile
            serverId={serverId}
            tile={tile}
            backgrounded={!shown}
            mirror={mirror}
            avatar={tile.user === undefined ? null : avatarOf(tile.user)}
            menu={renderTileMenu?.(tile)}
          />
        )}
      />
      <WithoutCamera people={model.withoutCamera} avatarOf={avatarOf} />
      <RoomBar
        serverId={serverId}
        onAir={model.own?.state === "onAir"}
        selfHidden={model.selfHidden}
        onTurnOn={onTurnOn}
        background={background}
        onPopOut={() => {
          popOut(
            "camera",
            { serverId, session: null, plugin: null },
            title,
            tc("media.popOutFailed"),
          );
        }}
        fullscreen={fullscreen}
        onFullscreen={toggle}
        onClose={closeCameraRoom}
      />
    </section>
  );
}
