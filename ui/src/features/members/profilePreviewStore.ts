/**
 * State of the one shared profile preview (FR-1.18, QA.21). Triggers report hover and focus here;
 * `ProfilePreviewHost` renders whatever is open. There is a single timer for the whole app (open
 * delay or close grace), so long lists never leave per-row timers running.
 */

/** Hover or focus time before the preview opens. */
export const PREVIEW_OPEN_DELAY_MS = 400;
/** Time to move the pointer between the person and the preview before it closes. */
export const PREVIEW_CLOSE_GRACE_MS = 150;
/** Id of the preview element; there is only ever one, so it can be fixed. */
export const PREVIEW_ID = "profile-preview";

export interface OpenPreview {
  /** Session of the person; `null` for someone who is not on the server (history). */
  session: number | null;
  /** Name stored with a chat message; used to find the person and as the fallback label. */
  name?: string | undefined;
  /** Element the preview is placed next to. */
  anchor: HTMLElement;
  /** Element that gets `aria-describedby` pointing at the preview (the focused one, if any). */
  described: HTMLElement;
}

let current: OpenPreview | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();

function clearTimer() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
}

function describedBy(element: HTMLElement, on: boolean) {
  const ids = (element.getAttribute("aria-describedby") ?? "").split(" ").filter((id) => id !== "");
  const next = on ? [...new Set([...ids, PREVIEW_ID])] : ids.filter((id) => id !== PREVIEW_ID);
  if (next.length > 0) element.setAttribute("aria-describedby", next.join(" "));
  else element.removeAttribute("aria-describedby");
}

function set(next: OpenPreview | null) {
  if (current?.described !== next?.described) {
    if (current) describedBy(current.described, false);
    if (next) describedBy(next.described, true);
  }
  current = next;
  for (const listener of listeners) listener();
}

export function subscribePreview(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function currentPreview(): OpenPreview | null {
  return current;
}

/**
 * The pointer or focus arrived on a person. Opens after the delay; while another preview is open
 * it switches right away, so moving down a list doesn't wait at every row.
 */
export function requestPreview(
  session: number | null,
  anchor: HTMLElement,
  described = anchor,
  name?: string,
): void {
  clearTimer();
  if (current?.session === session && current.name === name && current.anchor === anchor) return;
  const next = { session, name, anchor, described };
  if (current) set(next);
  else
    timer = setTimeout(() => {
      timer = undefined;
      set(next);
    }, PREVIEW_OPEN_DELAY_MS);
}

/** The pointer or focus left the person or the preview: close after a short grace time. */
export function releasePreview(): void {
  clearTimer();
  if (!current) return;
  timer = setTimeout(() => {
    timer = undefined;
    set(null);
  }, PREVIEW_CLOSE_GRACE_MS);
}

/** The pointer is on the preview itself: keep it open. */
export function holdPreview(): void {
  clearTimer();
}

/** Close now (click, Escape, scroll, window blur, person gone). */
export function closePreview(): void {
  clearTimer();
  if (current) set(null);
}

/**
 * Shows the preview for keyboard focus on `focused` (placed next to `anchor`). For rows that take
 * focus themselves (tree items); pair with `onBlur={releasePreview}`.
 */
export function previewOnFocus(
  session: number | null,
  anchor: HTMLElement,
  focused = anchor,
  name?: string,
): void {
  // Focus from a mouse click is not keyboard focus; the click opens the full card instead.
  if (focused.matches(":focus-visible")) requestPreview(session, anchor, focused, name);
}
