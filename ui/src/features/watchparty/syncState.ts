/**
 * How far each person in the party is from the party (issue #163). Players report where they are
 * (engine `positions`); the state is read at the moment the view was made and moves on at the same
 * rate as the party while both play, so a person stays behind until they report again.
 */
import type { ParticipantPosition } from "../../api";

/** Further than this from the party shows "N s behind", not "in sync". */
export const SYNC_TOLERANCE_MS = 2000;
/** A player that has not reported for this long is not shown as a state. */
export const STALE_MS = 30000;

export type SyncState =
  | { kind: "synced" }
  | { kind: "buffering" }
  | { kind: "behind"; seconds: number }
  | { kind: "ahead"; seconds: number }
  | { kind: "unknown" };

/**
 * The state of `report` against the party at `nowMs`. `partyAtMs` is when the party position was
 * read (the same anchor as the report), `partyMs` the party position then.
 */
export function syncState(
  report: ParticipantPosition,
  partyMs: number,
  partyAtMs: number,
  nowMs: number,
): SyncState {
  if (report.buffering) return { kind: "buffering" };
  if (report.ageMs > STALE_MS) return { kind: "unknown" };
  const elapsed = nowMs - partyAtMs;
  const theirs = report.positionMs + (report.playing ? elapsed : 0);
  const drift = partyMs + elapsed - theirs;
  if (Math.abs(drift) <= SYNC_TOLERANCE_MS) return { kind: "synced" };
  const seconds = Math.round(Math.abs(drift) / 1000);
  return drift > 0 ? { kind: "behind", seconds } : { kind: "ahead", seconds };
}

/** How many people the sync pill has to name as not in sync (issue #163). */
export function outOfSync(states: readonly SyncState[]): number {
  return states.filter((state) => state.kind !== "synced").length;
}
