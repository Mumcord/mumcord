/** How the own camera reaches its viewers (FR-8.62, FR-8.94). */
import { describe, expect, it } from "vitest";
import type { CameraView } from "../../../api";
import { ownCameraPath } from "./ownCameraPath";

function view(patch: Partial<CameraView>): CameraView {
  return {
    serverId: 1,
    state: "onAir",
    error: null,
    broadcast: "00112233aabbccdd",
    viewers: [],
    ...patch,
  };
}

describe("own camera path", () => {
  it("knows nothing while the camera is off", () => {
    expect(ownCameraPath(null)).toBeNull();
    expect(ownCameraPath(view({ state: "off" }))).toBeNull();
    expect(ownCameraPath(view({ state: "preview" }))).toBeNull();
  });

  it("names the path the camera uses when nobody watches yet", () => {
    expect(ownCameraPath(view({}))).toBe("relay");
    expect(ownCameraPath(view({ broadcast: null }))).toBe("direct");
  });

  it("names the path every viewer gets", () => {
    const relay = view({ viewers: [{ session: 2, path: "relay" }] });
    expect(ownCameraPath(relay)).toBe("relay");
    const direct = view({ viewers: [{ session: 2, path: "direct" }] });
    expect(ownCameraPath(direct)).toBe("direct");
  });

  it("says both while an old relay serves 0.8.x viewers beside direct ones", () => {
    const mixed = view({
      viewers: [
        { session: 2, path: "relay" },
        { session: 3, path: "direct" },
      ],
    });
    expect(ownCameraPath(mixed)).toBe("both");
  });
});
