import { describe, expect, it } from "vitest";
import en from "../../locales/en/wizard.json";
import {
  GUIDE_TARGETS,
  guideOffset,
  guideShift,
  guideSpot,
  poseFor,
  type Box,
} from "./wizardGuideLayout";

const AARON: Box = { left: 800, top: 200, right: 960, bottom: 360 };

function box(left: number, top: number, width = 100, height = 40): Box {
  return { left, top, right: left + width, bottom: top + height };
}

describe("poseFor", () => {
  it("stands idle without a target", () => {
    expect(poseFor(null, AARON)).toBe("idle");
  });

  it("points toward a target beside him", () => {
    expect(poseFor(box(300, 250), AARON)).toBe("point-left");
    expect(poseFor(box(1100, 250), AARON)).toBe("point-right");
  });

  it("points up at a target above him", () => {
    expect(poseFor(box(830, 40), AARON)).toBe("point-up");
  });

  it("stands idle when the target is right below him", () => {
    expect(poseFor(box(830, 500), AARON)).toBe("idle");
  });
});

describe("guideOffset", () => {
  const column: Box = { left: 800, top: 100, right: 960, bottom: 700 };

  it("levels his arm with the target's middle", () => {
    expect(guideOffset(box(300, 300, 100, 40), column, 115, 300)).toBe(320 - 100 - 115);
  });

  it("stays inside the column", () => {
    expect(guideOffset(box(300, 0), column, 115, 300)).toBe(0);
    expect(guideOffset(box(300, 1000), column, 115, 300)).toBe(300);
  });

  it("starts at the top without a target", () => {
    expect(guideOffset(null, column, 115, 300)).toBe(0);
  });
});

describe("guideShift", () => {
  const column: Box = { left: 1200, top: 0, right: 1440, bottom: 900 };

  it("steps out of his column to just right of the target", () => {
    expect(guideShift(box(300, 300, 400), [], column, 16)).toBe(700 + 16 - 1200);
  });

  it("never stands over other content beside the target", () => {
    expect(guideShift(box(300, 300, 400), [box(750, 250, 400)], column, 16)).toBe(1150 + 16 - 1200);
  });

  it("stays in his column when the content reaches it, or without a target", () => {
    expect(guideShift(box(300, 300, 1000), [], column, 16)).toBe(0);
    expect(guideShift(null, [], column, 16)).toBe(0);
  });
});

describe("guideSpot", () => {
  const column: Box = { left: 1200, top: 0, right: 1440, bottom: 900 };
  // A neighbour card beside the top of a tall target, like the identity step's import card.
  const neighbour = box(750, 100, 400, 60);
  const besideAt = (top: number, bottom: number) =>
    neighbour.bottom > top && neighbour.top < bottom ? [neighbour] : [];

  it("stands just below content that blocks him, if he still points at the target", () => {
    const tall = box(300, 100, 400, 300);
    expect(guideSpot(tall, column, 115, 300, 16, besideAt)).toEqual({
      offset: 160 + 16,
      shift: 700 + 16 - 1200,
    });
  });

  it("stays level with a short target that the content blocks", () => {
    const short = box(300, 110, 400, 40);
    expect(guideSpot(short, column, 115, 300, 16, besideAt)).toEqual({
      offset: 130 - 115,
      shift: 1150 + 16 - 1200,
    });
  });
});

describe("texts", () => {
  it("has one English title and hint for every step", () => {
    const steps = Object.keys(GUIDE_TARGETS).sort();
    expect(Object.keys(en.guide.hints).sort()).toEqual(steps);
    expect(Object.keys(en.guide.titles).sort()).toEqual(steps);
  });

  it("points at the microphone choice on the voice step", () => {
    expect(GUIDE_TARGETS.voice).toBe(".wizard-voice__devices select");
  });
});
