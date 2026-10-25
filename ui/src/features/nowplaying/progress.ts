/** Live playback position of a shared track (FR-1.58). */
import type { NowPlaying } from "../../api";

/** A shared track and the local time (ms) its `positionMs` was valid. */
export interface LiveTrack {
  track: NowPlaying;
  atMs: number;
}

/**
 * The position at `nowMs` (local clock): moves on from `atMs` while playing and stops at the end
 * of the track. Mirrors the engine's `extrapolate`.
 */
export function positionAt({ track, atMs }: LiveTrack, nowMs: number): number {
  const elapsed = track.playing ? Math.max(0, nowMs - atMs) : 0;
  const position = track.positionMs + elapsed;
  return track.durationMs === null ? position : Math.min(position, track.durationMs);
}

/** "m:ss", or "h:mm:ss" from an hour on. */
export function formatTime(ms: number): string {
  const total = Math.floor(Math.max(0, ms) / 1000);
  const seconds = String(total % 60).padStart(2, "0");
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  return hours > 0
    ? `${String(hours)}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${String(minutes)}:${seconds}`;
}

/** "Artist – Title", or the title alone. */
export function trackLabel(track: Pick<NowPlaying, "artist" | "title">): string {
  return track.artist ? `${track.artist} – ${track.title}` : track.title;
}
