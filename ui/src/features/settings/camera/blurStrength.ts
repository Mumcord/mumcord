// Same values as the preview dialog (ui/src/features/camera/preview/previewLogic.ts); use those
// once both have landed.

/** Light is the engine's default blur strength (`DEFAULT_CAMERA_BLUR_STRENGTH`). */
export const BLUR_LIGHT = 50;
/** Strong is the engine's maximum blur strength. */
export const BLUR_STRONG = 100;

/** Older or hand-edited settings may hold any 0–100 value; above Light counts as Strong. */
export function isStrongBlur(strength: number): boolean {
  return strength > BLUR_LIGHT;
}
