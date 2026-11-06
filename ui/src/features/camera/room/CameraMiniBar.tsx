/**
 * "{n} cameras on · Open room" at the top of the chat (FR-8.53, FR-8.61): while cameras are on in
 * the channel and the room is closed. Opening it joins the cameras again. Below it, while the
 * room bar is not on screen (room closed, or open behind another item), the "Camera stopped: …"
 * notice of the own camera (section 5.6), also when no camera is left on.
 */
import { useTranslation } from "react-i18next";
import { MiniBar } from "../../media/MiniBar";
import { openMedia } from "../../media/mediaView";
import { useCameraRoomShown, useRoomBarOnScreen } from "./useCameraRoomShown";
import { CameraStoppedNotice } from "./CameraStoppedNotice";
import { RoomIcon } from "./icons";
import { useCameraRoom, useOwnCamera } from "./roomStore";

export function CameraMiniBar({ serverId }: { serverId: number }) {
  const { t } = useTranslation("camera");
  const room = useCameraRoom(serverId);
  const own = useOwnCamera(serverId);
  const shown = useCameraRoomShown(serverId);
  const count = (room?.tiles.length ?? 0) + (own?.state === "onAir" ? 1 : 0);
  const barOnScreen = useRoomBarOnScreen(serverId);
  // The notice shows here whenever the room bar, its other place, is not on screen.
  const notice = barOnScreen ? null : <CameraStoppedNotice serverId={serverId} />;
  if (shown || count === 0) return notice;
  const text = t("room.miniBar", { count });
  return (
    <>
      <MiniBar
        label={t("room.label", { count })}
        title={text}
        thumbnail={<RoomIcon name="camera" size={16} />}
        status={null}
        openLabel={t("room.open")}
        onOpen={() => {
          openMedia("camera");
        }}
      />
      {notice}
    </>
  );
}
