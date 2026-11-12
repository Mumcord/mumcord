import { describe, expect, it } from "vitest";
import { AIM_DEPTH, CORNER_BOTTOM, EDGE, GAP, HAND_Y, placeGuide } from "./placement";

const viewport = { width: 1280, height: 800 };
const guide = { width: 450, height: 200, mascotWidth: 112, mascotHeight: 100 };
/** Hand height when Aaron stands at the top of the guide, and at the bottom. */
const handTop = 100 * HAND_Y;
const handBottom = 200 - 100 * (1 - HAND_Y);

describe("placeGuide", () => {
  it("waits idle in the lower right corner without a target", () => {
    expect(placeGuide(null, guide, viewport)).toEqual({
      left: 1280 - 450 - EDGE,
      top: 800 - 200 - CORNER_BOTTOM,
      pose: "idle",
      mascot: "right",
      align: "end",
    });
  });

  it.each([
    ["has no size", { left: 10, top: 10, width: 0, height: 0 }],
    ["is off-screen", { left: -400, top: 10, width: 300, height: 40 }],
  ])("treats a target that %s as missing", (_why, target) => {
    expect(placeGuide(target, guide, viewport).pose).toBe("idle");
  });

  it("stands right of a target on the left and points near the top of a tall one", () => {
    const serverBar = { left: 0, top: 0, width: 72, height: 800 };
    expect(placeGuide(serverBar, guide, viewport)).toEqual({
      left: 72 + GAP,
      top: AIM_DEPTH - handTop,
      pose: "point-left",
      mascot: "left",
      align: "start",
    });
  });

  it("stands at the bottom of the guide next to a target in the lower half", () => {
    const panel = { left: 0, top: 500, width: 240, height: 40 };
    expect(placeGuide(panel, guide, viewport)).toEqual({
      left: 240 + GAP,
      top: 520 - handBottom,
      pose: "point-left",
      mascot: "left",
      align: "end",
    });
  });

  it("stands left of a target on the right and points right", () => {
    const apps = { left: 1100, top: 300, width: 40, height: 40 };
    expect(placeGuide(apps, guide, viewport)).toEqual({
      left: 1100 - GAP - 450,
      top: 320 - handTop,
      pose: "point-right",
      mascot: "right",
      align: "start",
    });
  });

  it("stands below a target with no room beside it and points up", () => {
    const wide = { left: 300, top: 40, width: 700, height: 40 };
    expect(placeGuide(wide, guide, viewport)).toEqual({
      left: 650 - guide.mascotWidth / 2,
      top: 80 + GAP,
      pose: "point-up",
      mascot: "left",
      align: "start",
    });
  });

  it("stands above a wide target at the bottom and points towards it", () => {
    const composer = { left: 300, top: 720, width: 700, height: 60 };
    expect(placeGuide(composer, guide, viewport)).toEqual({
      left: 650 - 450,
      top: 720 - GAP - 200,
      pose: "point-right",
      mascot: "right",
      align: "end",
    });
  });

  it("keeps the guide inside the window", () => {
    const panel = { left: 0, top: 740, width: 240, height: 60 };
    const placement = placeGuide(panel, guide, viewport);
    expect(placement.pose).toBe("point-left");
    expect(placement.align).toBe("end");
    expect(placement.top).toBe(800 - 200 - EDGE);

    const tiny = { width: 300, height: 150 };
    const cramped = placeGuide({ left: 100, top: 50, width: 100, height: 50 }, guide, tiny);
    expect(cramped.left).toBe(EDGE);
    expect(cramped.top).toBe(EDGE);
  });
});
