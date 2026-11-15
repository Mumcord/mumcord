/**
 * How the own camera reaches its viewers (FR-8.62, FR-8.94): through the relay broadcast, over one
 * direct stream per viewer, or both at once — an old relay serves 0.8.x viewers through its
 * broadcast while 0.9.0 viewers get a stream of their own. `null` while the own camera is off.
 */
import type { CameraView } from "../../../api";

export type OwnCameraPath = "relay" | "direct" | "both";

export function ownCameraPath(view: CameraView | null): OwnCameraPath | null {
  if (view === null || view.state !== "onAir") return null;
  const direct = view.viewers.some((viewer) => viewer.path === "direct");
  const relay = view.viewers.some((viewer) => viewer.path === "relay");
  if (direct && relay) return "both";
  if (direct) return "direct";
  if (relay) return "relay";
  // Nobody watches yet: the path the camera uses is the one it has (FR-8.94).
  return view.broadcast === null ? "direct" : "relay";
}
