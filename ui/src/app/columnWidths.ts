/**
 * Widths of the resizable channel and member columns (FR-1.15, QA.29), in rem so they follow the
 * user's text scaling. Pure functions plus the small storage wrapper; the shell owns the state.
 */

export type ResizableColumn = "channels" | "members";

export type ColumnWidths = Record<ResizableColumn, number>;

export const COLUMN_MIN_REM = 15;
export const COLUMN_MAX_REM = 28;
/** The channel tree starts 25% wider than the minimum; the member list starts at the minimum. */
export const COLUMN_DEFAULT_REM: Record<ResizableColumn, number> = {
  channels: COLUMN_MIN_REM * 1.25,
  members: COLUMN_MIN_REM,
};
/** One arrow key press moves the border by this much. */
export const COLUMN_STEP_REM = 1;
/** The chat column keeps at least this much room (360px at the default text size). */
export const CHAT_MIN_REM = 22.5;

export const DEFAULT_COLUMN_WIDTHS: ColumnWidths = {
  channels: COLUMN_DEFAULT_REM.channels,
  members: COLUMN_DEFAULT_REM.members,
};

/** What the window leaves for the columns, measured in rem. */
export interface ShellRoom {
  /** Width of the whole shell. */
  shell: number;
  /** Width of the server bar. */
  serverBar: number;
  /** False on narrow windows, where the member list is an overlay outside the grid. */
  membersShown: boolean;
}

export function clampWidth(rem: number, max = COLUMN_MAX_REM): number {
  return Math.min(Math.max(rem, COLUMN_MIN_REM), Math.max(COLUMN_MIN_REM, max));
}

/** Room for both side columns together once the server bar and the chat minimum are taken. */
function sideRoom(room: ShellRoom): number {
  return room.shell - room.serverBar - CHAT_MIN_REM;
}

/**
 * The widths actually shown: the chosen ones, shrunk so the chat column keeps its minimum. The
 * member list gives way first. Below the minimum widths nothing shrinks further; the window's
 * own minimum size keeps that case from overflowing.
 */
export function fitWidths(chosen: ColumnWidths, room: ShellRoom): ColumnWidths {
  let channels = clampWidth(chosen.channels);
  let members = clampWidth(chosen.members);
  const space = sideRoom(room);
  if (!room.membersShown) return { channels: clampWidth(channels, space), members };
  const overflow = channels + members - space;
  if (overflow > 0) {
    members = clampWidth(members - overflow);
    channels = clampWidth(channels, space - members);
  }
  return { channels, members };
}

/** Largest width a column can take right now without squeezing the chat or the other column. */
export function maxWidth(column: ResizableColumn, shown: ColumnWidths, room: ShellRoom): number {
  const other = column === "channels" ? (room.membersShown ? shown.members : 0) : shown.channels;
  return clampWidth(sideRoom(room) - other);
}

/**
 * New width for a key press on a column's resize handle, or null for keys it ignores. The arrow
 * keys move the border in their direction, so for the member list (border on its left) the left
 * arrow widens it. Home and End jump to the smallest and largest width.
 */
export function widthForKey(
  key: string,
  column: ResizableColumn,
  current: number,
  max: number,
): number | null {
  const widen = column === "channels" ? "ArrowRight" : "ArrowLeft";
  const narrow = column === "channels" ? "ArrowLeft" : "ArrowRight";
  switch (key) {
    case widen:
      return clampWidth(current + COLUMN_STEP_REM, max);
    case narrow:
      return clampWidth(current - COLUMN_STEP_REM, max);
    case "Home":
      return COLUMN_MIN_REM;
    case "End":
      return clampWidth(COLUMN_MAX_REM, max);
    default:
      return null;
  }
}

/** Width while dragging: the start width plus the pointer movement towards the chat column. */
export function widthForDrag(
  column: ResizableColumn,
  start: number,
  deltaPx: number,
  remPx: number,
  max: number,
): number {
  const delta = deltaPx / remPx;
  return clampWidth(start + (column === "channels" ? delta : -delta), max);
}

/*
 * A pure layout preference of this window, so it lives in the webview's storage rather than the
 * engine's settings record. Storage can be unavailable (private mode, quota); the app then simply
 * starts with the default widths.
 */
const STORAGE_KEY = "mumcord.columnWidths";

export function loadColumnWidths(storage?: Storage): ColumnWidths {
  try {
    const raw = (storage ?? localStorage).getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_COLUMN_WIDTHS;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return DEFAULT_COLUMN_WIDTHS;
    const record = parsed as Partial<Record<ResizableColumn, unknown>>;
    const read = (value: unknown, column: ResizableColumn) =>
      typeof value === "number" && Number.isFinite(value)
        ? clampWidth(value)
        : COLUMN_DEFAULT_REM[column];
    return {
      channels: read(record.channels, "channels"),
      members: read(record.members, "members"),
    };
  } catch (error) {
    console.warn("Reading the column widths failed; using the defaults", error);
    return DEFAULT_COLUMN_WIDTHS;
  }
}

export function saveColumnWidths(widths: ColumnWidths, storage?: Storage): void {
  try {
    (storage ?? localStorage).setItem(STORAGE_KEY, JSON.stringify(widths));
  } catch (error) {
    console.warn("Saving the column widths failed", error);
  }
}
