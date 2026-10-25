/**
 * Zoom and pan math of the image viewer. A view is the image's scale (screen pixels per image
 * pixel) and the offset of its centre from the centre of the stage; both are applied as one CSS
 * transform, so nothing re-lays out while zooming.
 */
export interface View {
  scale: number;
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 16;
/** Factor of one "+" / "−" step (key or button). */
export const ZOOM_STEP = 1.25;

/** Small images are enlarged to fit at most this much, so a tiny picture does not become a blur. */
export const MAX_FIT = 4;

/** Scale at which the whole image fits the stage; 1 while a size is still unknown. */
export function fitScale(stage: Size, image: Size): number {
  if (stage.width <= 0 || stage.height <= 0 || image.width <= 0 || image.height <= 0) return 1;
  return Math.min(MAX_FIT, stage.width / image.width, stage.height / image.height);
}

export function fitView(stage: Size, image: Size): View {
  return { scale: fitScale(stage, image), x: 0, y: 0 };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Keeps at least a margin of the image on the stage so it cannot be dragged away. */
export function clampView(view: View, stage: Size, image: Size): View {
  const margin = 48;
  const limitX = Math.max(0, (image.width * view.scale + stage.width) / 2 - margin);
  const limitY = Math.max(0, (image.height * view.scale + stage.height) / 2 - margin);
  return {
    scale: view.scale,
    x: clamp(view.x, -limitX, limitX),
    y: clamp(view.y, -limitY, limitY),
  };
}

/**
 * Multiplies the scale by `factor` so that the image point under `pointer` (relative to the
 * stage's centre) stays under it.
 */
export function zoomAround(
  view: View,
  factor: number,
  pointer: { x: number; y: number },
  stage: Size,
  image: Size,
): View {
  const scale = clamp(view.scale * factor, MIN_ZOOM, MAX_ZOOM);
  const ratio = scale / view.scale;
  return clampView(
    {
      scale,
      x: pointer.x - (pointer.x - view.x) * ratio,
      y: pointer.y - (pointer.y - view.y) * ratio,
    },
    stage,
    image,
  );
}

/** Scale factor of a wheel event; touchpad pinch arrives as ctrl+wheel with small deltas. */
export function wheelFactor(deltaY: number, deltaMode: number, ctrlKey: boolean): number {
  const pixels = deltaMode === 1 ? deltaY * 16 : deltaY;
  return Math.exp(-pixels * (ctrlKey ? 0.01 : 0.0015));
}

/** Whether the view still shows the fit-to-window state (so a resize keeps fitting). */
export function isFit(view: View, stage: Size, image: Size): boolean {
  return view.x === 0 && view.y === 0 && Math.abs(view.scale - fitScale(stage, image)) < 1e-6;
}
