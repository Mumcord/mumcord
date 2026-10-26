/**
 * Where a tooltip goes (FR-2.79): beside its trigger on the preferred side, flipped to the
 * opposite side when that one has no room, then shifted so it stays inside the window. Pure
 * functions in CSS pixels so the maths can be tested without a layout engine.
 */

export type Placement = "top" | "bottom" | "left" | "right";

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

/** Space between the trigger and the tooltip, so the tooltip never covers the click target. */
export const GAP = 6;

/** Minimum distance between the tooltip and the window edge. */
export const MARGIN = 8;

const OPPOSITE: Record<Placement, Placement> = {
  top: "bottom",
  bottom: "top",
  left: "right",
  right: "left",
};

export function parsePlacement(value: string | undefined): Placement {
  return value === "bottom" || value === "left" || value === "right" ? value : "top";
}

function fits(anchor: Box, tip: Size, viewport: Size, side: Placement): boolean {
  switch (side) {
    case "top":
      return anchor.top - GAP - tip.height >= MARGIN;
    case "bottom":
      return anchor.top + anchor.height + GAP + tip.height <= viewport.height - MARGIN;
    case "left":
      return anchor.left - GAP - tip.width >= MARGIN;
    case "right":
      return anchor.left + anchor.width + GAP + tip.width <= viewport.width - MARGIN;
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

export function placeTooltip(
  anchor: Box,
  tip: Size,
  viewport: Size,
  preferred: Placement,
): { left: number; top: number; placement: Placement } {
  const flipped = OPPOSITE[preferred];
  const placement =
    !fits(anchor, tip, viewport, preferred) && fits(anchor, tip, viewport, flipped)
      ? flipped
      : preferred;
  let left: number;
  let top: number;
  if (placement === "top" || placement === "bottom") {
    left = anchor.left + anchor.width / 2 - tip.width / 2;
    top = placement === "top" ? anchor.top - GAP - tip.height : anchor.top + anchor.height + GAP;
  } else {
    top = anchor.top + anchor.height / 2 - tip.height / 2;
    left = placement === "left" ? anchor.left - GAP - tip.width : anchor.left + anchor.width + GAP;
  }
  return {
    placement,
    left: clamp(left, MARGIN, viewport.width - MARGIN - tip.width),
    top: clamp(top, MARGIN, viewport.height - MARGIN - tip.height),
  };
}
