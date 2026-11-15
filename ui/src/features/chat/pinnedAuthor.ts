/**
 * First partly visible message in document order. Binary search avoids measuring all the older
 * rows on every scroll. A missing ref can occur during a render; use the safe linear path then.
 */
export function firstVisibleIndex(items: readonly (HTMLElement | null)[], listTop: number): number {
  let low = 0;
  let high = items.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    const item = items[middle];
    if (!item) {
      return items.findIndex((row) => row != null && row.getBoundingClientRect().bottom > listTop);
    }
    if (item.getBoundingClientRect().bottom > listTop) high = middle;
    else low = middle + 1;
  }
  return low < items.length ? low : -1;
}

/**
 * FR-2.62: the first visible message decides the pinned author. Find its group's header and last
 * row without measuring every row between them: only those two boundaries affect the banner.
 */
export function pinnedIndex(
  items: readonly (HTMLElement | null)[],
  listTop: number,
  bannerHeight: number,
): number | null {
  const topmost = firstVisibleIndex(items, listTop);
  if (topmost < 0) return null;
  let start = topmost;
  let header = items[start]?.querySelector(".chat-message__header");
  while (start >= 0 && items[start] && !header) {
    start -= 1;
    header = items[start]?.querySelector(".chat-message__header");
  }
  if (!header || header.getBoundingClientRect().bottom > listTop) return null;
  let end = topmost;
  while (items[end + 1] && !items[end + 1]?.querySelector(".chat-message__header")) end += 1;
  const last = items[end];
  if (!last || last.getBoundingClientRect().bottom <= listTop + bannerHeight) return null;
  return start;
}
