import { describe, expect, it } from "vitest";
import { GAP, MARGIN, placeTooltip } from "./tooltipPlacement";

const viewport = { width: 800, height: 600 };
const tip = { width: 120, height: 30 };

describe("tooltip placement", () => {
  it("centres the tooltip on the preferred side with a gap", () => {
    const place = placeTooltip(
      { left: 400, top: 300, width: 40, height: 40 },
      tip,
      viewport,
      "top",
    );
    expect(place).toEqual({ placement: "top", left: 360, top: 300 - GAP - 30 });
  });

  it("flips to the other side when the preferred one has no room", () => {
    const right = placeTooltip(
      { left: 740, top: 300, width: 40, height: 40 },
      tip,
      viewport,
      "right",
    );
    expect(right.placement).toBe("left");
    expect(right.left).toBe(740 - GAP - 120);
  });

  it("keeps the preferred side when neither side has room", () => {
    const place = placeTooltip({ left: 0, top: 0, width: 40, height: 600 }, tip, viewport, "top");
    expect(place.placement).toBe("top");
    expect(place.top).toBe(MARGIN);
  });

  it("shifts along the edge so it never leaves the window", () => {
    const left = placeTooltip({ left: 0, top: 300, width: 20, height: 20 }, tip, viewport, "top");
    expect(left.left).toBe(MARGIN);
    const right = placeTooltip(
      { left: 790, top: 300, width: 10, height: 10 },
      tip,
      viewport,
      "bottom",
    );
    expect(right.left).toBe(800 - MARGIN - 120);
    const low = placeTooltip({ left: 20, top: 580, width: 20, height: 20 }, tip, viewport, "right");
    expect(low.top).toBe(600 - MARGIN - 30);
  });
});
