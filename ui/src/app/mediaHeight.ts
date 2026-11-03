/**
 * Height of the resizable media area above the chat (spec 07 §4.9, FR-7.61–7.74). The chosen
 * height is a share of the chat column's height, so it follows the window; "auto" is the look
 * from before the handle existed (16:9, at most 58 % of the window height). Pure functions plus
 * the small storage wrapper; `mediaResize.ts` owns the state.
 */

/** A share of the chat column height (0–1), or the original automatic size. */
export type MediaHeight = number | "auto";

/** Smallest height: this share of the chat column, but never under `MIN_REM`. */
export const MIN_SHARE = 0.2;
export const MIN_REM = 9;
/** Largest height: this share of the chat column, as long as the chat keeps `CHAT_KEEP_REM`. */
export const MAX_SHARE = 0.7;
/** The composer plus about two messages. */
export const CHAT_KEEP_REM = 13;
/** Below this chat column height there is no room for a handle (FR-7.71). */
export const HANDLE_ROOM_REM = 28;
/** Soft snap points as shares of the chat column, and how close the pointer has to come. */
export const SNAPS = [1 / 3, 1 / 2, 2 / 3] as const;
export const SNAP_PX = 12;
/** Dragging the picture below this share of the chat column offers to minimise it. */
export const COLLAPSE_SHARE = 0.12;
/** One arrow key press moves the handle by this much. */
export const STEP_REM = 2;

/** What the chat column leaves for the picture, measured in pixels. */
export interface MediaRoom {
  /** Height of the chat column. */
  column: number;
  /**
   * Everything in the column above the handle that is not the picture (channel header, bars,
   * padding, the watching strip) plus the handle itself. The chat gets what is left below.
   */
  overhead: number;
  /** Pixels per rem right now (follows the text size). */
  remPx: number;
}

export interface MediaLimits {
  min: number;
  max: number;
}

/** The hard minimum and maximum picture height in pixels, or null when there is no room. */
export function mediaLimits(room: MediaRoom): MediaLimits | null {
  if (room.column < HANDLE_ROOM_REM * room.remPx) return null;
  const min = Math.max(room.column * MIN_SHARE, MIN_REM * room.remPx);
  const max = Math.min(
    room.column * MAX_SHARE,
    room.column - CHAT_KEEP_REM * room.remPx - room.overhead,
  );
  return max < min ? null : { min, max };
}

const clamp = (px: number, limits: MediaLimits) => Math.min(Math.max(px, limits.min), limits.max);

/** The snap points that lie between the limits, in pixels, from small to large. */
export function snapPoints(room: MediaRoom, limits: MediaLimits): number[] {
  return SNAPS.map((share) => share * room.column).filter(
    (px) => px >= limits.min && px <= limits.max,
  );
}

/** Height shown for a chosen share: shrunk to the limits, never rewritten (FR-7.71). */
export function shownHeight(chosen: number, room: MediaRoom, limits: MediaLimits): number {
  return clamp(chosen * room.column, limits);
}

export interface DragResult {
  /** Picture height in pixels. */
  px: number;
  /** True when the height sits on a snap point. */
  snapped: boolean;
  /** The pointer is far enough below the minimum that releasing minimises the area. */
  collapse: boolean;
}

/** Height for a pointer drag: raw height in pixels, soft snaps unless Alt is held. */
export function heightForDrag(
  rawPx: number,
  room: MediaRoom,
  limits: MediaLimits,
  noSnap: boolean,
): DragResult {
  const collapse = rawPx < room.column * COLLAPSE_SHARE;
  const px = clamp(rawPx, limits);
  if (!noSnap) {
    const snap = snapPoints(room, limits).find((point) => Math.abs(point - px) <= SNAP_PX);
    if (snap !== undefined) return { px: snap, snapped: true, collapse };
  }
  return { px, snapped: false, collapse };
}

/**
 * New height for a key press on the handle, "auto" for Enter, or null for keys it ignores. The
 * keys move the handle in their direction (ARIA window splitter), so Up makes the picture smaller.
 */
export function heightForKey(
  key: string,
  currentPx: number,
  room: MediaRoom,
  limits: MediaLimits,
): number | "auto" | null {
  const step = STEP_REM * room.remPx;
  switch (key) {
    case "ArrowUp":
      return clamp(currentPx - step, limits);
    case "ArrowDown":
      return clamp(currentPx + step, limits);
    case "PageUp": {
      const below = snapPoints(room, limits).filter((point) => point < currentPx - 0.5);
      return below.at(-1) ?? limits.min;
    }
    case "PageDown":
      return snapPoints(room, limits).find((point) => point > currentPx + 0.5) ?? limits.max;
    case "Home":
      return limits.min;
    case "End":
      return limits.max;
    case "Enter":
      return "auto";
    default:
      return null;
  }
}

/** A picture height in pixels as the whole percent of the chat column shown to the user. */
export function percentOf(px: number, room: MediaRoom): number {
  return Math.round((px / room.column) * 100);
}

/*
 * A layout preference of this window like the column widths (`columnWidths.ts`), so it lives in
 * the webview's storage next to them. One value for every channel and every media view.
 */
const STORAGE_KEY = "mumcord.mediaHeight";

export function loadMediaHeight(storage?: Storage): MediaHeight {
  try {
    const raw = (storage ?? localStorage).getItem(STORAGE_KEY);
    if (raw === null) return "auto";
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === "number" && Number.isFinite(parsed) && parsed > 0 && parsed <= 1
      ? parsed
      : "auto";
  } catch (error) {
    console.warn("Reading the media area height failed; using Auto", error);
    return "auto";
  }
}

export function saveMediaHeight(height: MediaHeight, storage?: Storage): void {
  try {
    (storage ?? localStorage).setItem(STORAGE_KEY, JSON.stringify(height));
  } catch (error) {
    console.warn("Saving the media area height failed", error);
  }
}
