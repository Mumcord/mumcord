/**
 * The camera room (spec 08 §4.8, mockups 05–07): a dark media card with the header, the tiles in
 * the Grid or Speaker layout, the people without a camera and the room bar. It renders in the
 * media area of the main window and in the camera pop-out; it keeps its dark picture background
 * in both themes (FR-8.70).
 */
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useAvatars } from "../../members/useAvatars";
import { closeMedia } from "../../media/mediaView";
import { popOut, usePopoutTitle } from "../../media/popoutStore";
import { useStageShown } from "../../media/stageContext";
import { useStageLabel } from "../../media/stageRegistry";
import { useFullscreen } from "../../media/useFullscreen";
import { CameraTile } from "./CameraTile";
import { RoomBackground } from "./RoomBackground";
import { RoomBar } from "./RoomBar";
import { RoomHeader } from "./RoomHeader";
import { forgetTileChoices } from "./roomChoices";
import { RoomStage } from "./RoomStage";
import { TileMenu } from "./TileMenu";
import { useRoomModel } from "./useRoomModel";
import { useCameraAnnouncements } from "./useCameraAnnouncements";
import { useCameraLook } from "./useCameraLook";
import { WithoutCamera } from "./WithoutCamera";
import "./camera.css";

export interface CameraRoomProps {
  serverId: number;
  /** Opens the preview dialog; `null` where it cannot open (the pop-out). */
  onTurnOn: (() => void) | null;
}

/** Closes the room: the media area lets it go and the per-tile choices are forgotten. */
function closeCameraRoom(): void {
  forgetTileChoices();
  closeMedia("camera");
}

export function CameraRoom({ serverId, onTurnOn }: CameraRoomProps) {
  const { t } = useTranslation("camera");
  const { t: tc } = useTranslation("common");
  const model = useRoomModel(serverId);
  const shown = useStageShown();
  const { mirror, reduceMotion } = useCameraLook();
  const ref = useRef<HTMLElement>(null);
  const { fullscreen, toggle } = useFullscreen(ref);
  // The bar's background picker; the self-view menu's "Background" opens it as well (FR-8.40).
  const [pickingBackground, setPickingBackground] = useState(false);
  const onAir = model.own?.state === "onAir";
  if (!onAir && pickingBackground) setPickingBackground(false);
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
        renderTile={(tile, _place, tabIndex) => (
          <CameraTile
            serverId={serverId}
            tile={tile}
            backgrounded={!shown}
            mirror={mirror}
            avatar={tile.user === undefined ? null : avatarOf(tile.user)}
            tabIndex={tabIndex}
            menu={
              <TileMenu
                serverId={serverId}
                tile={tile}
                large={model.large}
                onBackground={
                  onAir
                    ? () => {
                        setPickingBackground(true);
                      }
                    : null
                }
              />
            }
          />
        )}
      />
      <WithoutCamera people={model.withoutCamera} avatarOf={avatarOf} />
      <RoomBar
        serverId={serverId}
        onAir={onAir}
        selfHidden={model.selfHidden}
        onTurnOn={onTurnOn}
        background={
          <RoomBackground open={pickingBackground && onAir} onOpenChange={setPickingBackground} />
        }
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
