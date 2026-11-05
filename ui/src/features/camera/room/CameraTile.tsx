/**
 * One camera tile (FR-8.65, spec 08 §5.3): the video, the name tag with the microphone icon, the
 * green speaking frame, "● Live" on the self-view, the "{n} fps" badge of a weak connection and,
 * with the tile under the pointer or focused, the height and codec it arrives in (mockup 10).
 * Without a picture it shows who it is and why: joining, full, couldn't join or hidden (§7.2).
 * Names come from the server and are rendered as text only.
 */
import type { TFunction } from "i18next";
import { useCallback, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { MockPicture } from "../../media/MockPicture";
import { mockPlaceholder } from "../../media/mockPlaceholder";
import { Avatar } from "../../members/Avatar";
import { codecName } from "../../screenshare/shareFormat";
import { RoomIcon } from "./icons";
import type { TileModel } from "./useRoomModel";
import { useTileFrames, useTileHeight, type TileFormat } from "./useTileFrames";

/** Mock mode draws no frames; its tiles show the format a real 720p AV1 camera would. */
const MOCK_FORMAT: TileFormat = { height: 720, codec: "av1" };

export interface CameraTileProps {
  serverId: number;
  tile: TileModel;
  /** The room is a stage tab in the background: keep decoding, draw later. */
  backgrounded: boolean;
  /** Mirror the self-view ("Mirror my self-view", FR-8.66). */
  mirror: boolean;
  avatar: string | null;
  /** The tile menu button (U-5); placed in the tile's top-left corner. */
  menu?: ReactNode;
  /** Roving focus of the tiles (spec 08 §5.5): only one tile is a Tab stop. */
  tabIndex?: 0 | -1;
}

export function CameraTile({
  serverId,
  tile,
  backgrounded,
  mirror,
  avatar,
  menu,
  tabIndex = 0,
}: CameraTileProps) {
  const { t } = useTranslation("camera");
  const label = tileLabel(tile, t);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const tileRef = useRef<HTMLDivElement>(null);
  const [pictured, setPictured] = useState(false);
  const [received, setReceived] = useState<TileFormat | null>(null);
  const self = tile.state === "self";
  const video = self || tile.state === "playing";
  const mock = mockPlaceholder() !== null;
  useTileFrames(
    canvasRef,
    self ? { kind: "self" } : { kind: "camera", serverId, sharer: tile.session },
    video && !mock,
    backgrounded,
    useCallback((shown: boolean) => {
      setPictured(shown);
      if (!shown) setReceived(null);
    }, []),
    setReceived,
  );
  const format = self || tile.state !== "playing" ? null : mock ? MOCK_FORMAT : received;
  useTileHeight(tileRef, serverId, self || !video ? null : tile.session);

  const status = statusText(tile, t);
  const showVideo = () => {
    getApi()
      .cameraTileSetHidden(serverId, tile.session, false)
      .catch((error: unknown) => {
        console.error("showing a camera again failed", error);
      });
  };

  return (
    <div
      ref={tileRef}
      className="cam-tile"
      role="group"
      tabIndex={tabIndex}
      aria-label={label}
      data-speaking={tile.speaking || undefined}
      data-tile={String(tile.key)}
    >
      <div className="cam-video" data-mirror={(self && mirror) || undefined}>
        {video &&
          (mock ? (
            <MockPicture label="" />
          ) : (
            <canvas ref={canvasRef} className="cam-video__canvas" aria-hidden="true" />
          ))}
      </div>
      {(status !== null || (video && !pictured && !mock)) && (
        <div className="cam-tile__off">
          <Avatar name={tile.name} src={avatar} size="md" />
          <span className="cam-tile__status">
            {status ?? t("tile.joining", { name: tile.name })}
          </span>
          {tile.state === "hidden" && (
            <button
              type="button"
              className="cam-btn cam-btn--small"
              // The tile is the Tab stop; H on it shows the video again.
              tabIndex={-1}
              onClick={showVideo}
            >
              <RoomIcon name="eye" />
              {t("tile.showVideo")}
            </button>
          )}
        </div>
      )}
      {menu}
      <span className="cam-tile__badges">
        {self && (
          <span className="cam-tile__badge" data-tone="live">
            ● {t("room.live")}
          </span>
        )}
        {format !== null && (
          <span className="cam-tile__badge" data-tone="format">
            {t("tile.format", { height: format.height, codec: codecName(format.codec) })}
          </span>
        )}
        {tile.fpsBadge !== null && video && (
          <span className="cam-tile__badge" data-tone="warn" data-tooltip={t("tile.weak")}>
            <RoomIcon name="weak" size={11} />
            {t("tile.fps", { fps: tile.fpsBadge })}
            <span className="visually-hidden">, {t("tile.weak")}</span>
          </span>
        )}
      </span>
      <span className="cam-tile__name">
        {tile.micOff ? (
          <span data-off>
            <RoomIcon name="micOff" size={12} />
          </span>
        ) : (
          tile.speaking && (
            <span data-talk>
              <RoomIcon name="mic" size={12} />
            </span>
          )
        )}
        <span className="cam-tile__label">{self ? t("room.you") : tile.name}</span>
      </span>
    </div>
  );
}

/** The screen-reader name of a tile (spec 08 §5.7). */
function tileLabel(tile: TileModel, t: TFunction<"camera">): string {
  if (tile.state === "self") return t("room.selfLabel");
  let label = t("tile.label", { name: tile.name });
  if (tile.speaking) label += t("tile.speaking");
  if (tile.micOff) label += t("tile.micOff");
  return label;
}

/** What a tile without a picture says (spec 08 §5.6, §7.2); `null` while it plays. */
function statusText(tile: TileModel, t: TFunction<"camera">): string | null {
  switch (tile.state) {
    case "announced":
    case "joining":
      return t("tile.joining", { name: tile.name });
    case "full":
      return t("tile.full", { name: tile.name });
    case "failed":
      return t("tile.failed", { name: tile.name });
    case "hidden":
      return t("tile.hidden");
    default:
      return null;
  }
}
