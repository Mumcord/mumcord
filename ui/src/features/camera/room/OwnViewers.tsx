/**
 * Who watches the own camera and how each of them gets it (FR-8.62, FR-8.94): through the relay
 * broadcast or over a direct stream of their own. A button in the room bar opens the list; it is
 * there only while the camera is on and somebody watches.
 */
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useServerStateOf } from "../../channels";
import { RoomIcon } from "./icons";
import { useOwnCamera } from "./roomStore";

export function OwnViewers({ serverId }: { serverId: number }) {
  const { t } = useTranslation("camera");
  const view = useOwnCamera(serverId);
  const { users } = useServerStateOf(serverId);
  const [open, setOpen] = useState(false);
  const listId = useId();
  const viewers = view?.viewers ?? [];
  if (viewers.length === 0) return null;
  // A viewer that left the channel already is not in the state any more; its number still says
  // which entry it was.
  const nameOf = (session: number) => users.get(session)?.name ?? `#${String(session)}`;
  return (
    <div className="cam-viewers">
      <button
        type="button"
        className="cam-btn"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => {
          setOpen(!open);
        }}
        onKeyDown={(event) => {
          // The list itself holds no focus: the button keeps it, so Escape works on the button.
          if (event.key === "Escape" && open) {
            event.preventDefault();
            setOpen(false);
          }
        }}
      >
        <RoomIcon name="eye" />
        {t("viewers.title")} ({viewers.length})
      </button>
      {open && (
        <ul className="cam-viewers__list" id={listId} aria-label={t("viewers.title")}>
          {viewers.map((viewer) => (
            <li key={viewer.session} className="cam-viewers__row">
              <span className="cam-viewers__name">{nameOf(viewer.session)}</span>
              <span className="cam-viewers__path" data-path={viewer.path}>
                {t(`viewers.${viewer.path}`)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
