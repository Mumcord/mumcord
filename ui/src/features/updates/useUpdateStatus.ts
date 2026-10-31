import { useEffect, useRef, useState } from "react";
import { getApi, type UpdateStatus } from "../../api";

/** Where a download started, to estimate the time left from the average speed since then. */
interface Sample {
  atMs: number;
  received: number;
}

/** Too short a stretch gives wild estimates; wait for this much data first. */
const MIN_SAMPLE_MS = 1000;

/** Seconds until `total` bytes are in, at the average speed since `start`; `null` if unknown. */
export function estimateSecondsLeft(
  start: Sample | null,
  nowMs: number,
  received: number | null,
  total: number | null,
): number | null {
  if (start === null || received === null || total === null) return null;
  const elapsedMs = nowMs - start.atMs;
  const gained = received - start.received;
  if (elapsedMs < MIN_SAMPLE_MS || gained <= 0) return null;
  return Math.max(0, Math.ceil(((total - received) * elapsedMs) / gained / 1000));
}

export interface UpdateState {
  /** `null` while loading. */
  status: UpdateStatus | null;
  /** Estimated time left while downloading. */
  secondsLeft: number | null;
}

/**
 * The updater's state: read once, then kept current by `updateStatusChanged`. `onPrompt` runs
 * when the engine asks the UI to show the update dialog.
 */
export function useUpdateStatus(onPrompt?: () => void): UpdateState {
  const [state, setState] = useState<UpdateState>({ status: null, secondsLeft: null });
  const promptRef = useRef(onPrompt);
  useEffect(() => {
    promptRef.current = onPrompt;
  }, [onPrompt]);

  useEffect(() => {
    let active = true;
    // An event is newer than the answer to the first read, which must not overwrite it.
    let heard = false;
    let start: Sample | null = null;
    const api = getApi();
    const unsubscribe = api.on("updateStatusChanged", ({ status, prompt }) => {
      heard = true;
      const received = status.progress?.received ?? null;
      if (status.phase !== "downloading" || received === null) start = null;
      else start ??= { atMs: Date.now(), received };
      const secondsLeft = estimateSecondsLeft(
        start,
        Date.now(),
        received,
        status.progress?.total ?? null,
      );
      setState({ status, secondsLeft });
      if (prompt) promptRef.current?.();
    });
    api.updateStatus().then(
      (status) => {
        if (active && !heard) setState({ status, secondsLeft: null });
      },
      (error: unknown) => {
        console.error("Reading the update status failed", error);
      },
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, []);

  return state;
}
