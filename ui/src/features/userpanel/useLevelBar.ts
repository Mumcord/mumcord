import { useEffect, useRef } from "react";
import { getApi } from "../../api";

/** Share of the new level taken per frame while the level falls, so the bar sinks gently. */
const DECAY = 0.4;

/**
 * Draws the live microphone level (`micLevel` events, about 20 per second) as a scaled bar. It
 * paints from `requestAnimationFrame` straight into the element, never through React state, and
 * stops asking for frames once the bar has settled at silence.
 */
export function useLevelBar() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let target = 0;
    let shown = 0;
    let frame: number | undefined;

    const paint = () => {
      frame = undefined;
      shown = target >= shown ? target : shown + (target - shown) * DECAY;
      if (shown < 0.005) shown = 0;
      el.style.transform = `scaleX(${shown.toFixed(3)})`;
      if (shown !== target) frame = requestAnimationFrame(paint);
    };

    const off = getApi().on("micLevel", (event) => {
      target = Math.min(1, Math.max(0, event.level ?? 0));
      frame ??= requestAnimationFrame(paint);
    });
    return () => {
      off();
      if (frame !== undefined) cancelAnimationFrame(frame);
    };
  }, []);
  return ref;
}
