import { getApi, type CrashReport } from "../../api";

/**
 * Taking the unseen crash reports marks them as seen, so it happens once per app run and every
 * mount shares the answer (React runs effects twice in development, and the notice may remount).
 */
let unseenOnce: Promise<CrashReport[]> | null = null;

export function takeUnseenOnce(): Promise<CrashReport[]> {
  unseenOnce ??= getApi().crashTakeUnseen();
  return unseenOnce;
}

/** For tests: forget the answer of the last run. */
export function resetUnseenOnce(): void {
  unseenOnce = null;
}
