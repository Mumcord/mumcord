/**
 * The header of the combined layout (FR-8.72, mockups 08–09): the main item's name, "{n} cameras
 * on" and the switch Cameras beside / Above / Hidden, a radio group (spec 08 §5.5).
 */
import { useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { CameraStripPosition } from "../../../api";
import { MediaIcon } from "../../media/icons";
import { MonitorIcon } from "../../screenshare/icons";
import type { StripMain } from "./StripState";
import { setStripPosition } from "./StripState";

const POSITIONS: readonly CameraStripPosition[] = ["beside", "above", "hidden"];

export interface StripHeaderProps {
  main: StripMain;
  /** The main item's name, e.g. "Jonas's screen" or "Watch party". */
  title: string;
  cameraCount: number;
  position: CameraStripPosition;
}

export function StripHeader({ main, title, cameraCount, position }: StripHeaderProps) {
  const { t } = useTranslation("camera");
  return (
    <header className="cam-room__head cam-combined__head">
      <span className="cam-room__icon" aria-hidden="true">
        {main === "share" ? <MonitorIcon /> : <MediaIcon name="play" />}
      </span>
      <span className="cam-room__titles">
        <h2 className="cam-room__title">{title}</h2>
        <span className="cam-room__sub">{t("room.cameras", { count: cameraCount })}</span>
      </span>
      <span className="cam-room__chips">
        <PositionSwitch position={position} />
      </span>
    </header>
  );
}

/** Beside / Above / Hidden: one tab stop, arrow keys move and choose. */
function PositionSwitch({ position }: { position: CameraStripPosition }) {
  const { t } = useTranslation("camera");
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const onKey = (event: KeyboardEvent) => {
    const step =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0) return;
    event.preventDefault();
    const at = (POSITIONS.indexOf(position) + step + POSITIONS.length) % POSITIONS.length;
    setStripPosition(POSITIONS[at] ?? "beside");
    buttons.current[at]?.focus();
  };
  return (
    <span className="cam-seg" role="radiogroup" aria-label={t("position.label")}>
      {POSITIONS.map((option, i) => (
        <button
          key={option}
          ref={(element) => {
            buttons.current[i] = element;
          }}
          type="button"
          role="radio"
          aria-checked={position === option}
          tabIndex={position === option ? 0 : -1}
          onKeyDown={onKey}
          onClick={() => {
            setStripPosition(option);
          }}
        >
          {t(`position.${option}`)}
        </button>
      ))}
    </span>
  );
}
