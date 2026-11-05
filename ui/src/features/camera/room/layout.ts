/**
 * Pure layout rules of the camera room (spec 08 §4.8): the grid shape for 1–9 cameras, the stable
 * tile order and who the Speaker layout shows large. No React, so the rules are tested directly.
 */

/** The room holds at most this many cameras (FR-8.62 "{n} of 9 cameras"). */
export const ROOM_MAX = 9;

/** A tile is the own self-view or another person's camera, keyed by their session. */
export type TileKey = "self" | number;

export interface GridShape {
  cols: number;
  rows: number;
}

/** FR-8.63: 1 → 1 × 1, 2 → 1 × 2, 3–4 → 2 × 2, 5–6 → 2 × 3, 7–9 → 3 × 3. */
export function gridShape(count: number): GridShape {
  if (count <= 1) return { cols: 1, rows: 1 };
  if (count === 2) return { cols: 2, rows: 1 };
  if (count <= 4) return { cols: 2, rows: 2 };
  if (count <= 6) return { cols: 3, rows: 2 };
  return { cols: 3, rows: 3 };
}

/**
 * Widest the grid gets without a chosen area height, in rem, as in the mockups (05: 2 cameras use
 * the full width, 06: 4 cameras 44 rem, 07: 9 cameras 58 rem). `null` is the full width.
 */
export function presetWidthRem(count: number): number | null {
  if (count <= 2) return null;
  if (count <= 4) return 44;
  return 58;
}

/**
 * Width of a grid of `shape` that fits `width` × `height` with 16:9 tiles and `gap` between them;
 * the tiles are letterboxed inside the area (FR-8.63).
 */
export function fitGridWidth(shape: GridShape, width: number, height: number, gap: number): number {
  const byWidth = (width - gap * (shape.cols - 1)) / shape.cols;
  const byHeight = ((height - gap * (shape.rows - 1)) / shape.rows) * (16 / 9);
  const tile = Math.max(0, Math.min(byWidth, byHeight));
  return tile * shape.cols + gap * (shape.cols - 1);
}

/**
 * Keeps the order in which cameras turned on (FR-8.63): known tiles keep their place, new ones go
 * last, tiles that left are dropped. Returns `previous` itself when nothing changed.
 */
export function stableOrder(
  previous: readonly TileKey[],
  present: readonly TileKey[],
): readonly TileKey[] {
  const now = new Set(present);
  const kept = previous.filter((key) => now.has(key));
  const known = new Set(kept);
  const added = present.filter((key) => !known.has(key));
  if (added.length === 0 && kept.length === previous.length) return previous;
  return [...kept, ...added];
}

/** Pinned tiles move first, in the order they were pinned; the rest keep their order. */
export function withPinnedFirst(
  order: readonly TileKey[],
  pinned: readonly TileKey[],
): readonly TileKey[] {
  if (pinned.length === 0) return order;
  const present = new Set(order);
  const first = pinned.filter((key) => present.has(key));
  const firstSet = new Set(first);
  return [...first, ...order.filter((key) => !firstSet.has(key))];
}

/** Speaking for this long makes a person the Speaker layout's large tile (FR-8.64). */
export const SPEAKER_HOLD_MS = 1500;

/**
 * The Speaker layout's large tile (FR-8.64): the pinned person, else the person who last spoke for
 * at least 1.5 s, else the one shown before, else the first tile.
 */
export function speakerTile(
  order: readonly TileKey[],
  pinned: readonly TileKey[],
  lastSpeaker: TileKey | null,
  previous: TileKey | null,
): TileKey | null {
  const present = new Set(order);
  const pin = pinned.find((key) => present.has(key));
  if (pin !== undefined) return pin;
  if (lastSpeaker !== null && present.has(lastSpeaker)) return lastSpeaker;
  if (previous !== null && present.has(previous)) return previous;
  return order[0] ?? null;
}

/**
 * The tile height a viewer reports for Auto (FR-8.55): device pixels rounded up to 180 / 360 /
 * 540 / 720, so small changes do not cause a new report.
 */
export function heightStep(devicePx: number): number {
  const step = Math.ceil(devicePx / 180) * 180;
  return Math.min(720, Math.max(180, step));
}
