import type { AaronPose } from "./Aaron";

/** A box in window coordinates, as `getBoundingClientRect` reports it. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The size of Aaron with his speech bubble. */
export interface GuideSize {
  width: number;
  height: number;
  /** Aaron's own size; he stands at one end of the guide. */
  mascotWidth: number;
  mascotHeight: number;
}

export interface Placement {
  left: number;
  top: number;
  pose: AaronPose;
  /** Which end of the guide Aaron stands at, so he is the part closest to the target. */
  mascot: "left" | "right";
  /**
   * Whether Aaron stands at the top or the bottom of the guide. Next to a target in the lower
   * half of the window he stands at the bottom, so the bubble grows upwards and both stay level
   * with the target instead of being pushed away from it by the window edge.
   */
  align: "start" | "end";
}

/** Space kept to the window edges. */
export const EDGE = 16;
/** Space between the target and the guide. */
export const GAP = 12;
/** In the corner the guide stays above the message bar, like the toasts. */
export const CORNER_BOTTOM = 88;
/** Height of Aaron's pointing hand, as a share of his own height. */
export const HAND_Y = 0.7;
/**
 * The deepest point into a target Aaron points at. Tall targets such as the server bar or the
 * channel list start with their most important content, so he points near their top instead of
 * at their empty middle.
 */
export const AIM_DEPTH = 120;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

function visible(target: Box, viewport: { width: number; height: number }): boolean {
  return (
    target.width > 0 &&
    target.height > 0 &&
    target.left < viewport.width &&
    target.top < viewport.height &&
    target.left + target.width > 0 &&
    target.top + target.height > 0
  );
}

/**
 * Where Aaron and his bubble float for a tip, and which way he points. He tries the right side
 * of the target first (most targets sit in the left columns), then the left side, then below it
 * pointing up. When none fits, as for the wide message bar, he stands above it at its centre and
 * points right towards it. Without a visible target he waits idle in the lower right corner.
 * The guide never overlaps the target and is always kept inside the window.
 */
export function placeGuide(
  target: Box | null,
  guide: GuideSize,
  viewport: { width: number; height: number },
): Placement {
  const maxLeft = Math.max(EDGE, viewport.width - guide.width - EDGE);
  const maxTop = Math.max(EDGE, viewport.height - guide.height - EDGE);

  if (target === null || !visible(target, viewport)) {
    return {
      left: maxLeft,
      top: clamp(viewport.height - guide.height - CORNER_BOTTOM, EDGE, maxTop),
      pose: "idle",
      mascot: "right",
      align: "end",
    };
  }

  const right = target.left + target.width;
  const bottom = target.top + target.height;
  const centreX = target.left + target.width / 2;
  // The hand level with the point Aaron aims at.
  const aimY = target.top + Math.min(target.height / 2, AIM_DEPTH);
  const align = aimY > viewport.height / 2 ? "end" : "start";
  const handY =
    align === "start"
      ? guide.mascotHeight * HAND_Y
      : guide.height - guide.mascotHeight * (1 - HAND_Y);
  const besideTop = clamp(aimY - handY, EDGE, maxTop);

  if (right + GAP + guide.width <= viewport.width - EDGE) {
    return { left: right + GAP, top: besideTop, pose: "point-left", mascot: "left", align };
  }
  if (target.left - GAP - guide.width >= EDGE) {
    return {
      left: target.left - GAP - guide.width,
      top: besideTop,
      pose: "point-right",
      mascot: "right",
      align,
    };
  }
  if (bottom + GAP + guide.height <= viewport.height - EDGE) {
    return {
      left: clamp(centreX - guide.mascotWidth / 2, EDGE, maxLeft),
      top: bottom + GAP,
      pose: "point-up",
      mascot: "left",
      align: "start",
    };
  }
  return {
    left: clamp(centreX - guide.width, EDGE, maxLeft),
    top: clamp(target.top - GAP - guide.height, EDGE, maxTop),
    pose: "point-right",
    mascot: "right",
    align: "end",
  };
}
