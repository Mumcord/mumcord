/** Vertical extent of one message, relative to the viewport; `headerBottom` is null when it has no author header. */
export interface PinItem {
  top: number;
  bottom: number;
  headerBottom: number | null;
}

/**
 * FR-2.62: index of the run's first message (the one with the author header) whose author belongs
 * in the banner at the top of the list, or null. The topmost partly visible message decides: it
 * either has its own header or is a grouped follow-up of the run before it. The banner shows once
 * that run's header has scrolled out of view and enough of the run remains below the banner.
 */
export function pinnedIndex(
  items: readonly (PinItem | null)[],
  listTop: number,
  bannerHeight: number,
): number | null {
  const topmost = items.findIndex((item) => item !== null && item.bottom > listTop);
  if (topmost < 0) return null;
  let start = topmost;
  while (start >= 0 && items[start]?.headerBottom === null) start -= 1;
  const header = items[start];
  if (!header || header.headerBottom === null || header.headerBottom > listTop) return null;
  let end = topmost;
  while (items[end + 1]?.headerBottom === null) end += 1;
  const last = items[end];
  if (!last || last.bottom <= listTop + bannerHeight) return null;
  return start;
}
