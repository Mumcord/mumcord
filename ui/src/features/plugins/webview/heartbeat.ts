/**
 * The frame watchdog's UI half (FR-9.42): the page's bootstrap sends a heartbeat every second;
 * after 3 s without one the frame is removed. Browsers slow down timers of hidden windows, so
 * the watch pauses while the app window or the frame is hidden and starts fresh when it shows.
 */

/** Silence after which a frame counts as stopped. */
export const HEARTBEAT_TIMEOUT_MS = 3000;
const CHECK_EVERY_MS = 500;

export interface HeartbeatWatch {
  /** A heartbeat arrived. */
  readonly beat: () => void;
  /** Pauses (`false`) or resumes with a fresh grace period (`true`). */
  readonly setActive: (active: boolean) => void;
  readonly stop: () => void;
}

export function watchHeartbeat(
  onStopped: () => void,
  now: () => number = () => performance.now(),
): HeartbeatWatch {
  let last = now();
  let active = true;
  let stopped = false;
  const timer = setInterval(() => {
    if (!active || stopped || document.visibilityState !== "visible") return;
    if (now() - last < HEARTBEAT_TIMEOUT_MS) return;
    stopped = true;
    clearInterval(timer);
    onStopped();
  }, CHECK_EVERY_MS);
  const onVisibility = () => {
    if (document.visibilityState === "visible") last = now();
  };
  document.addEventListener("visibilitychange", onVisibility);
  const stop = () => {
    stopped = true;
    clearInterval(timer);
    document.removeEventListener("visibilitychange", onVisibility);
  };
  return {
    beat: () => {
      last = now();
    },
    setActive: (next) => {
      if (next && !active) last = now();
      active = next;
    },
    stop,
  };
}
