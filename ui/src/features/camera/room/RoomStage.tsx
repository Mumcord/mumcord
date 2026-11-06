/**
 * The tiles of the camera room in the Grid (FR-8.63) or Speaker layout (FR-8.64). The tiles keep
 * 16:9 and are letterboxed inside the area: without a chosen media area height the grid takes the
 * mockups' width for its count (at most 58 % of the window high), with one it fits that height.
 * A tile chosen with "Show large" takes the Speaker layout's large place in either layout. The
 * tiles are one Tab stop with roving focus and their own keys (spec 08 §5.5).
 */
import {
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import { fitGridWidth, gridShape, presetWidthRem } from "./layout";
import { runTileCommand } from "./TileMenuActions";
import { useGridKeys } from "./useGridKeys";
import type { RoomModel, TileModel } from "./useRoomModel";

/** Gap between tiles (`--space-2`) in rem. */
const GAP_REM = 0.5;
/** Without a chosen area height the tiles take at most this share of the window height. */
const AUTO_HEIGHT_SHARE = 0.58;

interface Box {
  width: number;
  height: number;
  rem: number;
  /** The height is the area's chosen one (or full screen, the pop-out), not the Auto budget. */
  sized: boolean;
}

/** The stage's content box; its height only counts when the media area has a chosen height. */
function useBox(ref: RefObject<HTMLElement | null>): Box | null {
  const [box, setBox] = useState<Box | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const update = () => {
      const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
      const sized =
        element.closest(".media-area[data-sized]") !== null ||
        element.closest("[data-fullscreen], .media-popout") !== null;
      const next = {
        width: element.clientWidth,
        height: sized ? element.clientHeight : window.innerHeight * AUTO_HEIGHT_SHARE,
        rem,
        sized,
      };
      setBox((prev) =>
        prev !== null &&
        prev.width === next.width &&
        prev.height === next.height &&
        prev.rem === next.rem &&
        prev.sized === next.sized
          ? prev
          : next,
      );
    };
    update();
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(update) : null;
    observer?.observe(element);
    window.addEventListener("resize", update);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", update);
    };
  }, [ref]);
  return box;
}

export interface RoomStageProps {
  model: RoomModel;
  renderTile: (tile: TileModel, place: "grid" | "large" | "strip", tabIndex: 0 | -1) => ReactNode;
}

export function RoomStage({ model, renderTile }: RoomStageProps) {
  const { t } = useTranslation("camera");
  const ref = useRef<HTMLDivElement>(null);
  const box = useBox(ref);
  const speaker =
    model.tiles.length < 2
      ? null
      : (model.large ?? (model.layout === "speaker" ? model.speaker : null));
  const large = speaker === null ? undefined : model.tiles.find((tile) => tile.key === speaker);
  const shape = gridShape(model.tiles.length);
  // Reading order: the large tile comes first, then the strip.
  const keys =
    large === undefined
      ? model.tiles.map((tile) => tile.key)
      : [large.key, ...model.tiles.filter((tile) => tile !== large).map((tile) => tile.key)];
  const gridKeys = useGridKeys(
    keys,
    large === undefined ? shape.cols : 1,
    (key, command, element) => runTileCommand(model, key, command, element),
  );
  const place = (tile: TileModel, where: "grid" | "large" | "strip") =>
    renderTile(tile, where, gridKeys.tabIndexOf(tile.key));

  let content: ReactNode;
  if (large === undefined) {
    const preset = presetWidthRem(model.tiles.length);
    const width =
      box === null
        ? undefined
        : Math.min(
            preset === null ? box.width : preset * box.rem,
            fitGridWidth(shape, box.width, box.height, GAP_REM * box.rem),
          );
    content = (
      <div
        className="cam-grid"
        role="list"
        aria-label={t("room.cameras")}
        style={{ "--cols": shape.cols, width } as CSSProperties}
      >
        {model.tiles.map((tile) => (
          <div key={String(tile.key)} role="listitem" className="cam-grid__item">
            {place(tile, "grid")}
          </div>
        ))}
      </div>
    );
  } else {
    content = <SpeakerLayout box={box} large={large} others={model.tiles} place={place} />;
  }

  return (
    // The keys come from the focused tile inside (a focusable group); the stage only delegates.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      ref={ref}
      className="cam-room__stage media-frame__picture"
      onKeyDown={gridKeys.onKeyDown}
      onFocus={gridKeys.onFocus}
    >
      {content}
    </div>
  );
}

/** Widest the large tile gets without a chosen area height, in rem. */
const SPEAKER_AUTO_REM = 34;
/** Strip tiles below the large one are this wide (mockup 09's row strip), in rem. */
const STRIP_TILE_REM = 11;

function SpeakerLayout({
  box,
  large,
  others,
  place,
}: {
  box: Box | null;
  large: TileModel;
  others: readonly TileModel[];
  place: (tile: TileModel, where: "large" | "strip") => ReactNode;
}) {
  const { t } = useTranslation("camera");
  const strip = others.filter((tile) => tile.key !== large.key);
  // The strip row takes its tiles' height plus a gap; the large tile fits the rest.
  const stripHeight = box === null ? 0 : ((STRIP_TILE_REM * 9) / 16 + GAP_REM) * box.rem;
  const fit =
    box === null ? 0 : fitGridWidth({ cols: 1, rows: 1 }, box.width, box.height - stripHeight, 0);
  // In Auto large tile and strip take about the height of a four-camera grid (mockup 06), so the
  // chat keeps its room.
  const width =
    box === null ? undefined : box.sized ? fit : Math.min(fit, SPEAKER_AUTO_REM * box.rem);
  return (
    <div className="cam-speaker">
      <div className="cam-speaker__large" style={{ width }}>
        {place(large, "large")}
      </div>
      <div className="cam-strip cam-strip--row" role="list" aria-label={t("room.cameras")}>
        {strip.map((tile) => (
          <div key={String(tile.key)} role="listitem" className="cam-grid__item">
            {place(tile, "strip")}
          </div>
        ))}
      </div>
    </div>
  );
}
