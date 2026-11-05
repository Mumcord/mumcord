/**
 * Keeps the camera room of the voice server in step with the engine and the media area (FR-8.53,
 * FR-8.61, FR-8.68). Renders nothing; mounted once in the main window.
 *
 * - The engine opened the room ("Show cameras: Always") or the own camera went on air: the room
 *   comes into the media area (as a tab with a dot while something else is the main item).
 * - The room is open for the engine exactly while it shows: in the media area or its pop-out.
 *   Closing it (Close room, minimising the area) stops every download.
 * - The last camera went off: the room and its pop-out close.
 * - The pop-out closed while cameras are still on: the room comes back into the main window.
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { closeMedia, openMedia, useMediaView } from "../../media/mediaView";
import { popIn, usePopoutTitle, usePopouts } from "../../media/popoutStore";
import { forgetTileChoices } from "./roomChoices";
import { useCameraRoomShown } from "./useCameraRoomShown";
import { useCameraRoom, useOwnCamera } from "./roomStore";

export function CameraRoomSync({ serverId }: { serverId: number }): null {
  const { t } = useTranslation("camera");
  const { t: tc } = useTranslation("common");
  const room = useCameraRoom(serverId);
  const own = useOwnCamera(serverId);
  const view = useMediaView();
  const popout = usePopouts().camera.target;
  const shown = useCameraRoomShown(serverId);
  const onAir = own?.state === "onAir";
  const cameras = (room?.tiles.length ?? 0) + (onAir ? 1 : 0);
  const engineOpen = room?.open ?? null;

  const wasOpen = useRef<boolean | null>(null);
  const wasOnAir = useRef(false);
  const wasPopped = useRef(false);

  // The engine opened the room, or the own camera went on air (spec 08 §5.1).
  useEffect(() => {
    const opened = engineOpen === true && wasOpen.current !== true;
    const wentOnAir = onAir && !wasOnAir.current;
    if (engineOpen !== null) wasOpen.current = engineOpen;
    wasOnAir.current = onAir;
    if ((opened || wentOnAir) && !view.stage.includes("camera") && popout === null) {
      openMedia("camera");
    }
  }, [engineOpen, onAir, view.stage, popout]);

  // The room downloads exactly while it shows (FR-8.57).
  useEffect(() => {
    if (engineOpen === null || engineOpen === shown) return;
    getApi()
      .cameraRoomSetOpen(serverId, shown)
      .catch((error: unknown) => {
        console.error("opening or closing the camera room failed", error);
      });
    if (!shown) forgetTileChoices();
  }, [serverId, engineOpen, shown]);

  // No camera left: the room goes, also from its own window.
  useEffect(() => {
    if (room === null || cameras > 0) return;
    closeMedia("camera");
    if (popout?.serverId === serverId) popIn("camera", tc("media.popInFailed"));
  }, [room, cameras, popout, serverId, tc]);

  // The pop-out closed while cameras are on: the room docks again.
  useEffect(() => {
    const popped = popout?.serverId === serverId;
    if (wasPopped.current && !popped && cameras > 0) openMedia("camera");
    wasPopped.current = popped;
  }, [popout, serverId, cameras]);

  // Another voice server: this room is over.
  useEffect(
    () => () => {
      closeMedia("camera");
    },
    [serverId],
  );

  // The pop-out's window title follows the channel ("Camera room — Lobby").
  const title = usePopoutTitle(serverId, t("room.title"));
  const popped = popout?.serverId === serverId;
  const titled = useRef(title);
  useEffect(() => {
    if (!popped || titled.current === title) return;
    titled.current = title;
    getApi()
      .mediaPopoutSetTitle("camera", title)
      .catch((error: unknown) => {
        console.error("Renaming the pop-out window failed", error);
      });
  }, [popped, title]);
  return null;
}
