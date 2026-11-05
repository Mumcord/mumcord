/**
 * Camera marker next to a person whose camera is on, in the channel tree of the shown server
 * (FR-8.17, FR-8.18, mockup 05): a red icon for the own camera, a muted one for other people's.
 * The icon sits at the row's end like in the mockup; the row's accessible name comes from the
 * name element, so the spoken label goes there separately.
 */
import { useTranslation } from "react-i18next";
import { useShownServerId } from "../channels/serverStore";
import { useOwnCamera } from "./availability";
import { CameraIcon } from "./CameraIcon";
import { usePersonCameraOn } from "./room/roomStore";
import "./cameraEntry.css";

interface MarkerProps {
  self: boolean;
  /** The row's person; their tile in the camera room says whether their camera is on. */
  session: number;
}

function useCameraOn({ self, session }: MarkerProps): boolean {
  const shownServerId = useShownServerId();
  const own = useOwnCamera();
  const other = usePersonCameraOn(self ? null : shownServerId, session);
  return self ? own?.state === "onAir" && own.serverId === shownServerId : other;
}

export function CameraTreeIcon(props: MarkerProps) {
  const { self } = props;
  if (!useCameraOn(props)) return null;
  return (
    <span className="camera-tree-icon" data-self={self ? "" : undefined} aria-hidden="true">
      <CameraIcon size={14} />
    </span>
  );
}

/** The marker's spoken part, placed inside the row's name element. */
export function CameraTreeLabel(props: MarkerProps & { name: string }) {
  const { self, name } = props;
  const { t } = useTranslation("camera");
  if (!useCameraOn(props)) return null;
  return (
    <span className="visually-hidden">
      , {self ? t("cameraOnLabel") : t("personCameraOn", { name })}
    </span>
  );
}
