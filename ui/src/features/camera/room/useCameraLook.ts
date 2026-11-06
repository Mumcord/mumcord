/**
 * How the room draws the own camera and the speaking frame: "Mirror my self-view" (FR-8.66,
 * default on) and the camera's reduce-motion choice (spec 08 §5.7; the system setting is followed
 * by the stylesheet itself).
 */
export interface CameraLook {
  mirror: boolean;
  /** The app's switch forces the static speaking frame regardless of the system setting. */
  reduceMotion: boolean;
}

export function useCameraLook(): CameraLook {
  return { mirror: true, reduceMotion: false };
}
