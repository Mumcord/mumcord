/** Where the portalled list goes: below the button, or above it when the window has no room. */
export interface ListPlacement {
  left: number;
  width: number;
  maxHeight: number;
  /** Distance from the window top, when the list opens downwards. */
  top: number;
  /** Distance from the window bottom, when it opens upwards. */
  above: number | null;
}

/** Space the list keeps to the window edge and to its button. */
const GAP = 4;
/** Below this much room the list opens upwards instead. */
const MIN_BELOW = 160;

export function placeList(rect: DOMRect, viewport = window.innerHeight): ListPlacement {
  const below = viewport - rect.bottom - GAP;
  const above = rect.top - GAP;
  const goUp = below < MIN_BELOW && above > below;
  const maxHeight = Math.max(MIN_BELOW, Math.min(16 * 16, goUp ? above : below));
  return {
    left: rect.left,
    width: rect.width,
    maxHeight,
    top: rect.bottom + GAP,
    above: goUp ? viewport - rect.top + GAP : null,
  };
}

/**
 * Where the portalled list goes in the DOM. A modal `<dialog>` lives in the browser's top layer,
 * which no `z-index` can reach, so a select inside one has to keep its list there as well;
 * everywhere else the body is right, because a list inside a scrolling card would be clipped.
 */
export function listHost(trigger: Element | null): HTMLElement {
  return trigger?.closest("dialog[open]") ?? document.body;
}
