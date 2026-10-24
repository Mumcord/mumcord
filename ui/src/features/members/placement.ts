/** Space between the person and the preview, and between the preview and the window edge. */
const GAP_PX = 8;

/**
 * Viewport position for a box of `width`×`height` next to `anchor`: right of it, else left, else
 * below or above; always clamped inside the window. Never over the anchor, so clicks reach it.
 */
export function placeNextTo(
  anchor: Pick<DOMRect, "left" | "right" | "top" | "bottom">,
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
): { left: number; top: number } {
  const clamp = (value: number, size: number, viewport: number) =>
    Math.max(GAP_PX, Math.min(value, viewport - GAP_PX - size));
  const sideTop = clamp(anchor.top, height, viewportHeight);
  if (anchor.right + GAP_PX + width <= viewportWidth - GAP_PX) {
    return { left: anchor.right + GAP_PX, top: sideTop };
  }
  if (anchor.left - GAP_PX - width >= GAP_PX) {
    return { left: anchor.left - GAP_PX - width, top: sideTop };
  }
  const left = clamp(anchor.left, width, viewportWidth);
  const below = anchor.bottom + GAP_PX;
  const top =
    below + height <= viewportHeight - GAP_PX
      ? below
      : clamp(anchor.top - GAP_PX - height, height, viewportHeight);
  return { left, top };
}
