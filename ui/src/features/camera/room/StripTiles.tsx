/**
 * The small camera tiles of the combined layout (FR-8.73): a column beside the main picture or a
 * row above it. The tiles report their height like in the room, so the senders send 180 or 360
 * lines. They are one Tab stop with roving focus and the room's tile keys (spec 08 §5.5); F on a
 * focused tile shows it large in the main picture's place.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { CameraStripPosition } from "../../../api";
import type { GridKeys } from "./useGridKeys";
import type { TileModel } from "./useRoomModel";

export interface StripTilesProps {
  tiles: readonly TileModel[];
  position: Exclude<CameraStripPosition, "hidden">;
  keys: Pick<GridKeys, "onKeyDown" | "onFocus">;
  renderTile: (tile: TileModel) => ReactNode;
}

export function StripTiles({ tiles, position, keys, renderTile }: StripTilesProps) {
  const { t } = useTranslation("camera");
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the keys belong to the focused tiles inside.
    <div
      className={position === "above" ? "cam-strip cam-strip--row" : "cam-strip"}
      role="list"
      aria-label={t("room.cameras")}
      onKeyDown={keys.onKeyDown}
      onFocus={keys.onFocus}
    >
      {tiles.map((tile) => (
        <div key={String(tile.key)} role="listitem" className="cam-strip__item">
          {renderTile(tile)}
        </div>
      ))}
    </div>
  );
}
