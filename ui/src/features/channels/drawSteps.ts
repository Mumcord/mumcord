import { useEffect, useState } from "react";

/**
 * How many of `total` items to draw. With `stepwise`, drawing starts with `step` items and adds
 * `step` more per frame until all are drawn, so a long list never blocks the window in one go
 * (FR-2.24); otherwise, and once done, everything is drawn. A new `key` (another server or
 * channel) starts over.
 */
export function useDrawSteps(total: number, step: number, stepwise: boolean, key: string): number {
  const [steps, setSteps] = useState(() => ({ key, drawn: stepwise ? step : null }));
  let current = steps;
  if (steps.key !== key) {
    current = { key, drawn: stepwise ? step : null };
    setSteps(current);
  }
  const drawn = current.drawn;
  useEffect(() => {
    if (drawn === null) return;
    const frame = requestAnimationFrame(() => {
      setSteps((s) => ({ key: s.key, drawn: drawn + step >= total ? null : drawn + step }));
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [drawn, step, total]);
  return drawn === null ? total : Math.min(drawn, total);
}
