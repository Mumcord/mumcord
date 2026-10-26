/**
 * One-shot request that the next connection section opens expanded ("Connection info…" in the
 * person menu, FR-2.25a). Every other opening starts collapsed (FR-2.25b), so nothing is remembered.
 */
let requested = false;

export function requestConnectionExpanded(): void {
  requested = true;
}

/** Whether a request is pending; safe to call during render. */
export function peekConnectionExpandedRequest(): boolean {
  return requested;
}

/** Clears the request; call from an effect once the section has mounted. */
export function takeConnectionExpandedRequest(): boolean {
  const value = requested;
  requested = false;
  return value;
}
