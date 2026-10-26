/**
 * Geometry of floating widgets (FR-2.70): moving, resizing and keeping them inside the window.
 * Pure functions in CSS pixels so the maths can be tested without a layout engine.
 */

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

/** Edges and corners a widget can be resized from. */
export type Edge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

export const EDGES: readonly Edge[] = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

export const MIN_SIZE: Size = { width: 320, height: 180 };

/**
 * Strip at the bottom of the window widgets never enter, so the chat input stays usable
 * whatever the widget's position.
 */
export const INPUT_RESERVE = 72;

/** Pixels per arrow key press. */
export const KEY_STEP = 16;

export interface SizeRules {
  min: Size;
  /** Width / height of the content below the title bar; `undefined` resizes freely. */
  aspect?: number | undefined;
  /** Height of the title bar, which is not part of the aspect ratio. */
  chrome: number;
  /** Fixed height below the aspect-bound content (e.g. party controls); not part of the ratio. */
  extra?: number | undefined;
}

/** With an aspect ratio the minimum size is that of the content; title bar and extra come on top. */
function minHeightOf(rules: SizeRules): number {
  return rules.aspect === undefined
    ? rules.min.height
    : rules.min.height + rules.chrome + (rules.extra ?? 0);
}

/** The area widgets live in: the window without the input reserve. */
export function boundsOf(window: Size): Size {
  return { width: window.width, height: Math.max(0, window.height - INPUT_RESERVE) };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

function heightFor(width: number, rules: SizeRules): number {
  return rules.aspect === undefined ? 0 : width / rules.aspect + rules.chrome + (rules.extra ?? 0);
}

function widthFor(height: number, rules: SizeRules): number {
  return rules.aspect === undefined
    ? 0
    : (height - rules.chrome - (rules.extra ?? 0)) * rules.aspect;
}

/**
 * Fits a size between the minimum and the bounds, keeping the aspect ratio. When the bounds are
 * smaller than the minimum, the bounds win so the widget still fits the window.
 */
export function fitSize(size: Size, bounds: Size, rules: SizeRules): Size {
  let width = clamp(size.width, rules.min.width, bounds.width);
  const minHeight = minHeightOf(rules);
  let height = clamp(size.height, minHeight, bounds.height);
  if (rules.aspect === undefined) {
    return { width: Math.min(width, bounds.width), height: Math.min(height, bounds.height) };
  }
  height = heightFor(width, rules);
  if (height < minHeight) {
    height = minHeight;
    width = widthFor(height, rules);
  }
  if (width > bounds.width) {
    width = bounds.width;
    height = heightFor(width, rules);
  }
  if (height > bounds.height) {
    height = bounds.height;
    width = widthFor(height, rules);
  }
  return { width, height };
}

/** Moves a rectangle so it lies inside the bounds (top left wins if it cannot fit). */
export function placeInside(rect: Rect, bounds: Size): Rect {
  return {
    ...rect,
    x: clamp(rect.x, 0, bounds.width - rect.width),
    y: clamp(rect.y, 0, bounds.height - rect.height),
  };
}

/** Fits size and position into the bounds. */
export function clampRect(rect: Rect, bounds: Size, rules: SizeRules): Rect {
  return placeInside({ ...rect, ...fitSize(rect, bounds, rules) }, bounds);
}

export function moveRect(start: Rect, dx: number, dy: number, bounds: Size): Rect {
  return placeInside({ ...start, x: start.x + dx, y: start.y + dy }, bounds);
}

/**
 * Resizes from an edge or corner by a pointer delta. The opposite edge stays put; with an aspect
 * ratio the dominant direction decides the size and the other follows.
 */
export function resizeRect(
  start: Rect,
  edge: Edge,
  dx: number,
  dy: number,
  bounds: Size,
  rules: SizeRules,
): Rect {
  const west = edge.includes("w");
  const north = edge.includes("n");
  const horizontal = west || edge.includes("e");
  const vertical = north || edge.includes("s");
  // Room towards the dragged edges: the widget never grows past the bounds.
  const maxWidth = horizontal
    ? west
      ? start.x + start.width
      : bounds.width - start.x
    : bounds.width;
  const maxHeight = vertical
    ? north
      ? start.y + start.height
      : bounds.height - start.y
    : bounds.height;
  let width = horizontal ? start.width + (west ? -dx : dx) : start.width;
  let height = vertical ? start.height + (north ? -dy : dy) : start.height;

  if (rules.aspect !== undefined) {
    const byWidth = !vertical || (horizontal && Math.abs(dx) >= Math.abs(dy));
    if (byWidth) height = heightFor(width, rules);
    else width = widthFor(height, rules);
  }
  const size = fitSize({ width, height }, { width: maxWidth, height: maxHeight }, rules);
  const x = west ? start.x + start.width - size.width : start.x;
  const y = north ? start.y + start.height - size.height : start.y;
  return placeInside({ x, y, ...size }, bounds);
}
