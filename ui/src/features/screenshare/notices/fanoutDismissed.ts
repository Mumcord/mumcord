/** Which shares' fan-out notice the user dismissed with "Got it" (FR-7.47: once per share). */

/**
 * When the user said "Got it", by server, as the start time of that share. The bar can mount
 * again while the share runs (another channel opened), so the card's own state would forget it.
 */
const dismissed = new Map<number, number>();
/** The start time the UI derives for a share moves by the event latency; shares differ by more. */
const SAME_SHARE_MS = 2000;

export function wasDismissed(serverId: number, startedAt: number): boolean {
  const at = dismissed.get(serverId);
  return at !== undefined && Math.abs(at - startedAt) < SAME_SHARE_MS;
}

/** Test helper: shows the notice again for every share. */
export function resetFanoutNotices(): void {
  dismissed.clear();
}

export function dismissFanoutNotice(serverId: number, startedAt: number): void {
  dismissed.set(serverId, startedAt);
}
