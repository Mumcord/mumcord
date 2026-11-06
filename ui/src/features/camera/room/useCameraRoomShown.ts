import { useMediaView } from "../../media/mediaView";
import { usePopouts } from "../../media/popoutStore";
import { useStripMain } from "./StripState";

/** Whether the camera room of `serverId` shows anywhere: the media area or its pop-out. */
export function useCameraRoomShown(serverId: number): boolean {
  const view = useMediaView();
  const popped = usePopouts().camera.target?.serverId === serverId;
  return popped || (view.cinema !== null && view.stage.includes("camera"));
}

/**
 * Whether the room bar of `serverId` is on screen in this window: the pop-out has it, or the room
 * is the main item of the media area, or the cameras are the strip next to a share or party.
 * While another item is the main one, the room is open but its bar is not visible.
 */
export function useRoomBarOnScreen(serverId: number): boolean {
  const view = useMediaView();
  const popped = usePopouts().camera.target?.serverId === serverId;
  const strip = useStripMain(serverId) !== null;
  return popped || strip || (view.cinema === "camera" && view.stage.includes("camera"));
}
