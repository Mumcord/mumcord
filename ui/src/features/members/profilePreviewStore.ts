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
/**
 * Wait before a single click pins the card. A double click (DM) then never shows a card that
 * vanishes again: the second click cancels the pending pin. Short enough to feel instant.
 */
export const PIN_DELAY_MS = 200;

export interface OpenPreview {
  /** Session of the person; `null` for someone who is not on the server (history). */
  session: number | null;
  /** Name stored with a chat message; used to find the person and as the fallback label. */
  name?: string | undefined;
  /** Server the person is on; the shown server when omitted. */
  serverId?: number | undefined;
  /** Element the preview is placed next to. */
  anchor: HTMLElement;
  /** Element that gets `aria-describedby` pointing at the preview (the focused one, if any). */
  described: HTMLElement;
  /** Opened by a click: stays until Escape, an outside click or its close button. */
  pinned?: boolean | undefined;
  /** Element that gets focus back when a pinned card is closed with the keyboard or button. */
  opener?: HTMLElement | null | undefined;
}

let current: OpenPreview | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let pinTimer: ReturnType<typeof setTimeout> | undefined;
const listeners = new Set<() => void>();

function clearTimer() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
}

function clearPinTimer() {
  if (pinTimer !== undefined) clearTimeout(pinTimer);
  pinTimer = undefined;
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
  serverId?: number,
): void {
  clearTimer();
  if (current?.pinned) return;
  if (
    current?.session === session &&
    current.name === name &&
    current.serverId === serverId &&
    current.anchor === anchor
  )
    return;
  const next = { session, name, serverId, anchor, described };
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
  if (!current || current.pinned) return;
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
  clearPinTimer();
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
  serverId?: number,
): void {
  // Focus from a mouse click is not keyboard focus; the click opens the full card instead.
  if (focused.matches(":focus-visible")) requestPreview(session, anchor, focused, name, serverId);
}

/** Whether `session`/`name` is the person the open preview (hovered or pinned) is about. */
export function isPreviewOf(session: number | null, name?: string): boolean {
  return current !== null && current.session === session && current.name === name;
}

/**
 * Pointer pressed on a person: another person's preview makes way, the same person's stays so
 * the click can pin it without a flicker. A pinned card is never closed by its own person.
 */
export function pointerDownOnPerson(session: number | null, name?: string): void {
  if (!isPreviewOf(session, name)) closePreview();
}

/**
 * A single click on a person: pins their card (converting the hover preview if it is open).
 * `delayed` waits for a possible second click, see `PIN_DELAY_MS`; keyboard use pins at once.
 */
export function pinPreview(
  session: number | null,
  anchor: HTMLElement,
  name: string | undefined,
  delayed: boolean,
): void {
  clearPinTimer();
  const pin = () => {
    pinTimer = undefined;
    clearTimer();
    if (current?.pinned && current.session === session && current.name === name) return;
    set({ session, name, anchor, described: anchor, pinned: true, opener: anchor });
  };
  if (delayed) pinTimer = setTimeout(pin, PIN_DELAY_MS);
  else pin();
}

/** A second click arrived: the pending pin is dropped (the double click opens the chat). */
export function cancelPendingPin(): void {
  clearPinTimer();
}
