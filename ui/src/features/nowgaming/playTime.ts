/** Live play time of a shared game (FR-1.59). */
import type { NowGaming } from "../../api";

/** Game-controller outline (24×24, stroked), used next to people and in the user panel. */
export const GAMEPAD_PATH =
  "M7 7h10a4 4 0 0 1 4 4v2a3 3 0 0 1-5.4 1.8L14.5 13h-5l-1.1 1.8A3 3 0 0 1 3 13v-2a4 4 0 0 1 4-4zM7.5 9v3M6 10.5h3M15.5 10h.01M17.5 12h.01";

/** A shared game and the local time (ms) its `playedMs` was valid. */
export interface LiveGame {
  game: NowGaming;
  atMs: number;
}

/** The play time at `nowMs` (local clock), moving on from `atMs`. */
export function playedAt({ game, atMs }: LiveGame, nowMs: number): number {
  return game.playedMs + Math.max(0, nowMs - atMs);
}

/** Whole hours and minutes of a play time. */
export function playedParts(ms: number): { hours: number; minutes: number } {
  const totalMinutes = Math.floor(Math.max(0, ms) / 60_000);
  return { hours: Math.floor(totalMinutes / 60), minutes: totalMinutes % 60 };
}
