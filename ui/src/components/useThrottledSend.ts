import { useCallback, useEffect, useRef } from "react";

/**
 * Sends the first change at once, then at most one per `intervalMs`; the latest change is always
 * sent at the end of the interval, so the value the user lets go on is never lost. A change still
 * pending when the component unmounts is sent then.
 */
export function useThrottledSend(intervalMs: number): { schedule: (send: () => void) => void } {
  const state = useRef<{
    timer: ReturnType<typeof setTimeout> | null;
    latest: (() => void) | null;
  }>({
    timer: null,
    latest: null,
  });

  useEffect(
    () => () => {
      const s = state.current;
      if (s.timer) clearTimeout(s.timer);
      s.timer = null;
      s.latest?.();
      s.latest = null;
    },
    [],
  );

  const schedule = useCallback(
    (send: () => void) => {
      const s = state.current;
      if (s.timer) {
        s.latest = send;
        return;
      }
      const fire = (now: () => void) => {
        now();
        s.timer = setTimeout(() => {
          const next = s.latest;
          s.latest = null;
          s.timer = null;
          if (next) fire(next);
        }, intervalMs);
      };
      fire(send);
    },
    [intervalMs],
  );

  return { schedule };
}
