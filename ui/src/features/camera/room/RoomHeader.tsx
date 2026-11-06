/**
 * The camera room's header (FR-8.62): "Camera room", "{n} cameras on · {m} people in {channel}",
 * "{n} of 9 cameras" while the room is full, "Relay · end-to-end encrypted" and the layout switch
 * Grid / Speaker, a radio group (spec 08 §5.5).
 */
import { useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { RoomIcon } from "./icons";
import { ROOM_MAX } from "./layout";
import { setRoomLayout, type RoomLayout } from "./roomChoices";
import type { RoomModel } from "./useRoomModel";

const LAYOUTS: readonly RoomLayout[] = ["grid", "speaker"];

export function RoomHeader({ model }: { model: RoomModel }) {
  const { t } = useTranslation("camera");
  const cameras = t("room.cameras", { count: model.cameraCount });
  const people = t("room.people", { count: model.peopleCount });
  return (
    <header className="cam-room__head">
      <span className="cam-room__icon" aria-hidden="true">
        <RoomIcon name="camera" size={16} />
      </span>
      <span className="cam-room__titles">
        <h2 className="cam-room__title">{t("room.title")}</h2>
        <span className="cam-room__sub">
          {t("room.subtitle", { cameras, people, channel: model.channelName })}
        </span>
      </span>
      <span className="cam-room__chips">
        {model.cameraCount >= ROOM_MAX && (
          <span className="cam-chip" data-tooltip={t("room.fullTitle")}>
            {t("room.limit", { count: model.cameraCount, max: ROOM_MAX })}
          </span>
        )}
        <span className="cam-chip" data-tone="ok" data-tooltip={t("room.relayTitle")}>
          <RoomIcon name="relay" size={12} />
          {t("room.relay")}
        </span>
        <LayoutSwitch layout={model.layout} />
      </span>
    </header>
  );
}

/** Grid / Speaker as a radio group: one tab stop, arrow keys move and choose. */
function LayoutSwitch({ layout }: { layout: RoomLayout }) {
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
    const at = (LAYOUTS.indexOf(layout) + step + LAYOUTS.length) % LAYOUTS.length;
    const next = LAYOUTS[at] ?? "grid";
    setRoomLayout(next);
    buttons.current[at]?.focus();
  };
  return (
    <span className="cam-seg" role="radiogroup" aria-label={t("room.layout")}>
      {LAYOUTS.map((option, i) => (
        <button
          key={option}
          ref={(element) => {
            buttons.current[i] = element;
          }}
          type="button"
          role="radio"
          aria-checked={layout === option}
          tabIndex={layout === option ? 0 : -1}
          onKeyDown={onKey}
          onClick={() => {
            setRoomLayout(option);
          }}
        >
          {t(`room.${option}`)}
        </button>
      ))}
    </span>
  );
}
