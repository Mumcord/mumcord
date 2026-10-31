/**
 * Only one player under the chat messages plays at a time (issues #260, #262): starting an
 * audio file, a music embed or a video embed stops whichever played before.
 */
import { useEffect, useRef } from "react";

let current: { stop: () => void } | null = null;

/** Makes `player` the one that plays and stops the previous one; returns the release. */
export function claimPlayback(stop: () => void): () => void {
  const player = { stop };
  const previous = current;
  current = player;
  previous?.stop();
  return () => {
    if (current === player) current = null;
  };
}

/** While `playing`, this card holds playback; `stop` is called when another card takes it. */
export function useExclusivePlayback(playing: boolean, stop: () => void): void {
  const stopRef = useRef(stop);
  useEffect(() => {
    stopRef.current = stop;
  }, [stop]);
  useEffect(() => {
    if (!playing) return;
    return claimPlayback(() => {
      stopRef.current();
    });
  }, [playing]);
}
