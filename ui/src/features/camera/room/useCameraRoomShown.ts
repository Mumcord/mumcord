import { useMediaView } from "../../media/mediaView";
import { usePopouts } from "../../media/popoutStore";

/** Whether the camera room of `serverId` shows anywhere: the media area or its pop-out. */
export function useCameraRoomShown(serverId: number): boolean {
  const view = useMediaView();
  const popped = usePopouts().camera.target?.serverId === serverId;
  return popped || (view.cinema !== null && view.stage.includes("camera"));
}
