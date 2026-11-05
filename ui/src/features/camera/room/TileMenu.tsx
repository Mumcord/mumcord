/**
 * The ⋯ button of a camera tile (spec 08 §5.3, mockup 10) and its menu. The button stays out of
 * the Tab order: the tile is the tab stop and opens the same menu with Enter or Shift+F10.
 */
import { useTranslation } from "react-i18next";
import type { TileKey } from "./layout";
import { RoomIcon } from "./icons";
import { closeTileMenu, openTileMenu, useOpenTileMenu } from "./TileMenuActions";
import { TileMenuPopup } from "./TileMenuPopup";
import "./TileMenu.css";
import type { TileModel } from "./useRoomModel";

export interface TileMenuProps {
  serverId: number;
  tile: TileModel;
  /** The tile shown large, to offer "Leave large view" on it. */
  large: TileKey | null;
  /** Opens the background picker of the room bar; the self-view menu offers it when given. */
  onBackground: (() => void) | null;
}

export function TileMenu({ serverId, tile, large, onBackground }: TileMenuProps) {
  const { t } = useTranslation("camera");
  const open = useOpenTileMenu();
  const expanded = open?.key === tile.key;
  const label = t("tileMenu.options", { name: tile.name });
  return (
    <>
      <button
        type="button"
        className="cam-tile__more"
        tabIndex={-1}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={expanded}
        onClick={(event) => {
          if (expanded) closeTileMenu();
          else openTileMenu(tile.key, event.currentTarget);
        }}
      >
        <RoomIcon name="more" />
      </button>
      {expanded && (
        <TileMenuPopup
          serverId={serverId}
          tile={tile}
          large={large}
          label={label}
          opener={open.opener}
          onBackground={onBackground}
        />
      )}
    </>
  );
}
