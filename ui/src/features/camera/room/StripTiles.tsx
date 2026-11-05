/**
 * The small camera tiles of the combined layout (FR-8.73): a column beside the main picture or a
 * row above it. The tiles report their height like in the room, so the senders send 180 or 360
 * lines. F on a focused tile shows it large in the main picture's place (spec 08 §5.5).
 */
import type { KeyboardEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { CameraStripPosition } from "../../../api";
import type { TileKey } from "./layout";
import { showLarge } from "./StripState";
import type { TileModel } from "./useRoomModel";

export interface StripTilesProps {
  tiles: readonly TileModel[];
  position: Exclude<CameraStripPosition, "hidden">;
  renderTile: (tile: TileModel) => ReactNode;
}

/** The tile a key went to, read from the tile's `data-tile`. */
function tileKeyOf(target: EventTarget): TileKey | null {
  if (!(target instanceof HTMLElement) || !target.classList.contains("cam-tile")) return null;
  const raw = target.dataset.tile;
  if (raw === undefined) return null;
  if (raw === "self") return "self";
  const session = Number(raw);
  return Number.isInteger(session) ? session : null;
}

export function StripTiles({ tiles, position, renderTile }: StripTilesProps) {
  const { t } = useTranslation("camera");
  const onKey = (event: KeyboardEvent) => {
    if (event.key !== "f" && event.key !== "F") return;
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const key = tileKeyOf(event.target);
    if (key === null) return;
    // The media views take F for full screen; on a strip tile it means Show large.
    event.preventDefault();
    event.stopPropagation();
    showLarge(key);
  };
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the keys belong to the focused tiles inside.
    <div
      className={position === "above" ? "cam-strip cam-strip--row" : "cam-strip"}
      role="list"
      aria-label={t("room.cameras")}
      onKeyDown={onKey}
    >
      {tiles.map((tile) => (
        <div key={String(tile.key)} role="listitem" className="cam-strip__item">
          {renderTile(tile)}
        </div>
      ))}
    </div>
  );
}
