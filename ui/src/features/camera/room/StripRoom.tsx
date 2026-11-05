/**
 * The camera parts of the combined layout (spec 08 §5.4, FR-8.71–8.73): the strip header, the
 * small tiles beside or above the main picture, a strip tile shown large in the main picture's
 * place, and the room bar. They are grid items of the card `StripLayout` draws around the main
 * item, so the stylesheet places them by the strip position.
 */
import { useCallback, useEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { useAvatars } from "../../members/useAvatars";
import { closeMedia } from "../../media/mediaView";
import { popOut, usePopoutTitle } from "../../media/popoutStore";
import { useFullscreen } from "../../media/useFullscreen";
import { CameraTile } from "./CameraTile";
import { RoomBar } from "./RoomBar";
import type { TileKey } from "./layout";
import { forgetTileChoices, setLargeTile, useRoomChoices } from "./roomChoices";
import { StripHeader } from "./StripHeader";
import type { StripMain } from "./StripState";
import { useStripState } from "./StripState";
import { StripTiles } from "./StripTiles";
import { TileMenu } from "./TileMenu";
import { runTileCommand } from "./TileMenuActions";
import { useCameraAnnouncements } from "./useCameraAnnouncements";
import { useCameraLook } from "./useCameraLook";
import { useGridKeys } from "./useGridKeys";
import { useRoomModel, type TileModel } from "./useRoomModel";
import "./camera.css";

export interface StripRoomProps {
  serverId: number;
  main: StripMain;
  title: string;
  /** The card around main item and strip; full screen takes it. */
  frame: RefObject<HTMLElement | null>;
  /** Receive the header places the main item draws into (see `combinedHeader`). */
  subSlot: (element: HTMLElement | null) => void;
  chipsSlot: (element: HTMLElement | null) => void;
}

export function StripRoom({
  serverId,
  main,
  title: mainTitle,
  frame,
  subSlot,
  chipsSlot,
}: StripRoomProps) {
  const { t } = useTranslation("camera");
  const { t: tc } = useTranslation("common");
  const model = useRoomModel(serverId);
  const { position } = useStripState();
  // The room's own "Show large" choice, the only one (spec 08 §5.3); `model.large` is it while
  // that tile is still there.
  const chosenLarge = useRoomChoices().large;
  const large = model.large;
  const { mirror } = useCameraLook();
  const { fullscreen, toggle } = useFullscreen(frame);
  const avatarOf = useAvatars(
    model.tiles.flatMap((tile) => (tile.user === undefined ? [] : [tile.user])),
    serverId,
  );
  const title = usePopoutTitle(serverId, t("room.title"));
  useCameraAnnouncements(model.tiles);
  const hidden = position === "hidden";
  // The strip starts with the self-view (mockups 08–09); the others keep the room's order.
  const tiles = [
    ...model.tiles.filter((tile) => tile.key === "self"),
    ...model.tiles.filter((tile) => tile.key !== "self"),
  ];
  const largeTile = large === null ? undefined : model.tiles.find((tile) => tile.key === large);

  // Hidden unbinds every camera like Hide video; the header keeps counting them (FR-8.71).
  useEffect(() => {
    getApi()
      .cameraRoomSetVideosHidden(serverId, hidden)
      .catch((error: unknown) => {
        console.error("hiding or showing the strip's cameras failed", error);
      });
  }, [serverId, hidden]);
  // Leaving the combined layout shows every camera again.
  useEffect(
    () => () => {
      setLargeTile(null);
      getApi()
        .cameraRoomSetVideosHidden(serverId, false)
        .catch((error: unknown) => {
          console.error("showing the strip's cameras again failed", error);
        });
    },
    [serverId],
  );

  // The large tile's camera went off: the main picture comes back.
  useEffect(() => {
    if (chosenLarge !== null && largeTile === undefined) setLargeTile(null);
  }, [chosenLarge, largeTile]);

  // Escape leaves Show large (spec 08 §5.5); an open menu takes its own Escape first. Focus goes
  // back to the tile once it is in the strip again.
  const refocus = useRef<TileKey | null>(null);
  const leave = useCallback(() => {
    refocus.current = large;
    setLargeTile(null);
  }, [large]);
  useEffect(() => {
    const element = frame.current;
    if (element === null || large === null) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      leave();
    };
    element.addEventListener("keydown", onKey);
    return () => {
      element.removeEventListener("keydown", onKey);
    };
  }, [frame, large, leave]);
  useEffect(() => {
    const key = refocus.current;
    if (large !== null || key === null) return;
    refocus.current = null;
    frame.current?.querySelector<HTMLElement>(`.cam-strip [data-tile="${String(key)}"]`)?.focus();
  }, [frame, large]);

  // One Tab stop over the large tile and the strip, the large one first (spec 08 §5.5).
  const stripTiles = largeTile === undefined ? tiles : tiles.filter((tile) => tile !== largeTile);
  const gridKeys = useGridKeys(
    largeTile === undefined
      ? stripTiles.map((tile) => tile.key)
      : [largeTile.key, ...stripTiles.map((tile) => tile.key)],
    position === "above" ? stripTiles.length : 1,
    (key, command, element) => runTileCommand(model, key, command, element),
  );

  const renderTile = (tile: TileModel) => (
    <CameraTile
      serverId={serverId}
      tile={tile}
      backgrounded={false}
      mirror={mirror}
      avatar={tile.user === undefined ? null : avatarOf(tile.user)}
      tabIndex={gridKeys.tabIndexOf(tile.key)}
      menu={<TileMenu serverId={serverId} tile={tile} large={large} onBackground={null} />}
    />
  );

  return (
    <>
      <StripHeader
        main={main}
        title={mainTitle}
        cameraCount={model.cameraCount}
        position={position}
        subSlot={subSlot}
        chipsSlot={chipsSlot}
      />
      {!hidden && (
        <StripTiles
          tiles={stripTiles}
          position={position}
          keys={gridKeys}
          renderTile={renderTile}
        />
      )}
      {!hidden && largeTile !== undefined && (
        <>
          {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the keys belong to the focused tile inside. */}
          <div
            className="cam-combined__large"
            onKeyDown={gridKeys.onKeyDown}
            onFocus={gridKeys.onFocus}
          >
            {renderTile(largeTile)}
          </div>
          <button type="button" className="cam-combined__back" onClick={leave}>
            {t("tileMenu.leaveLarge")}
          </button>
        </>
      )}
      <RoomBar
        serverId={serverId}
        onAir={model.own?.state === "onAir"}
        selfHidden={model.selfHidden}
        onTurnOn={null}
        background={undefined}
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
        onClose={() => {
          forgetTileChoices();
          closeMedia("camera");
        }}
      />
    </>
  );
}
