/** Presentation of the overlay: optional channel name, then one row per person. */
import { useTranslation } from "react-i18next";
import { Avatar } from "../features/members/Avatar";

export interface OverlayPerson {
  session: number;
  name: string;
  avatar: string | null;
  talking: boolean;
  /** Stopped talking; fading out. */
  fading: boolean;
}

export function OverlayView({
  people,
  channelName,
  opacity,
  moveMode,
  onDone,
}: {
  people: readonly OverlayPerson[];
  channelName: string | null;
  opacity: number;
  moveMode: boolean;
  onDone: () => void;
}) {
  const { t } = useTranslation("overlay");
  // Idle: nothing to draw, the window stays fully transparent.
  if (!moveMode && people.length === 0) return null;
  return (
    <main
      className="overlay-panel"
      data-move-mode={moveMode}
      style={{ opacity }}
      // Move mode: dragging anywhere on the panel moves the window (Tauri drag region).
      data-tauri-drag-region={moveMode ? "" : undefined}
    >
      {moveMode && (
        <div className="overlay-panel__move" data-tauri-drag-region="">
          <span data-tauri-drag-region="">{t("move.hint")}</span>
          <button type="button" className="overlay-panel__done" onClick={onDone}>
            {t("move.done")}
          </button>
        </div>
      )}
      {channelName !== null && people.length > 0 && (
        <p className="overlay-panel__channel">{channelName}</p>
      )}
      <ul className="overlay-panel__people" aria-label={t("listLabel")}>
        {people.map((person) => (
          <li
            key={person.session}
            className="overlay-person"
            data-talking={person.talking}
            data-fading={person.fading}
          >
            <Avatar name={person.name} src={person.avatar} talking={person.talking} size="sm" />
            <span className="overlay-person__name">{person.name}</span>
          </li>
        ))}
      </ul>
    </main>
  );
}
