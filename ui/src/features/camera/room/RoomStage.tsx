/**
 * The tiles of the camera room in the Grid (FR-8.63) or Speaker layout (FR-8.64). The tiles keep
 * 16:9 and are letterboxed inside the area: without a chosen media area height the grid takes the
 * mockups' width for its count (at most 58 % of the window high), with one it fits that height.
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
  renderTile: (tile: TileModel, place: "grid" | "large" | "strip") => ReactNode;
}

export function RoomStage({ model, renderTile }: RoomStageProps) {
  const { t } = useTranslation("camera");
  const ref = useRef<HTMLDivElement>(null);
  const box = useBox(ref);
  const speaker = model.layout === "speaker" && model.tiles.length > 1 ? model.speaker : null;
  const large = speaker === null ? null : model.tiles.find((tile) => tile.key === speaker);

  let content: ReactNode;
  if (large === undefined || large === null) {
    const shape = gridShape(model.tiles.length);
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
            {renderTile(tile, "grid")}
          </div>
        ))}
      </div>
    );
  } else {
    content = (
      <SpeakerLayout box={box} large={large} others={model.tiles} renderTile={renderTile} />
    );
  }

  return (
    <div ref={ref} className="cam-room__stage media-frame__picture">
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
  renderTile,
}: {
  box: Box | null;
  large: TileModel;
  others: readonly TileModel[];
  renderTile: RoomStageProps["renderTile"];
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
        {renderTile(large, "large")}
      </div>
      <div className="cam-strip cam-strip--row" role="list" aria-label={t("room.cameras")}>
        {strip.map((tile) => (
          <div key={String(tile.key)} role="listitem" className="cam-grid__item">
            {renderTile(tile, "strip")}
          </div>
        ))}
      </div>
    </div>
  );
}
