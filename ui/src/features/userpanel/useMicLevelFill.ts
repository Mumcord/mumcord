import { useEffect, type RefObject } from "react";
import { getApi } from "../../api";

/** Minimum time between two paints (~16 fps): smooth enough, cheap for the UI thread. */
export const LEVEL_FRAME_MS = 60;
/** Level above which the reduced-motion tint switches on. */
export const LEVEL_THRESHOLD = 0.1;
/** Share of the new level taken per paint while the level falls, so the fill sinks gently. */
const DECAY = 0.4;

/**
 * Writes the live microphone level (`micLevel` events) to `--mic-level` on the element and
 * sets `data-active` above the threshold. It bypasses React state on purpose: about 20 events
 * per second must not re-render the panel.
 */
export function useMicLevelFill(ref: RefObject<HTMLElement | null>, enabled: boolean) {
  useEffect(() => {
    const el = ref.current;
    if (!enabled || !el) return;
    let target = 0;
    let shown = 0;
    let lastPaint = -Infinity;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const paint = () => {
      timer = undefined;
      lastPaint = performance.now();
      // Rise at once so speech shows immediately, fall smoothly to avoid flicker.
      shown = target >= shown ? target : shown + (target - shown) * DECAY;
      if (shown < 0.005) shown = 0;
      el.style.setProperty("--mic-level", shown.toFixed(3));
      if (shown >= LEVEL_THRESHOLD) el.dataset.active = "";
      else delete el.dataset.active;
    };

    const off = getApi().on("micLevel", (event) => {
      target = Math.min(1, Math.max(0, event.level ?? 0));
      if (timer !== undefined) return;
      const wait = Math.max(0, LEVEL_FRAME_MS - (performance.now() - lastPaint));
      timer = setTimeout(paint, wait);
    });

    return () => {
      off();
      clearTimeout(timer);
      el.style.removeProperty("--mic-level");
      delete el.dataset.active;
    };
  }, [ref, enabled]);
}
