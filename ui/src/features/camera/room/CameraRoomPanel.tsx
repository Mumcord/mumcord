/**
 * The camera room's place in the main window's media area (FR-8.61): the room itself, or the
 * "playing in a separate window" placeholder while it is popped out (FR-8.68 Pop out).
 */
import { useTranslation } from "react-i18next";
import { PoppedOut } from "../../media/PopoutControls";
import { usePopouts } from "../../media/popoutStore";
import { CameraRoom, type CameraRoomProps } from "./CameraRoom";

export function CameraRoomPanel(props: CameraRoomProps) {
  const { t } = useTranslation("camera");
  const target = usePopouts().camera.target;
  if (target?.serverId === props.serverId)
    return <PoppedOut kind="camera" title={t("room.title")} />;
  return <CameraRoom {...props} />;
}
