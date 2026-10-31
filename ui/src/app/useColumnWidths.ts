import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import {
  clampWidth,
  COLUMN_DEFAULT_REM,
  fitWidths,
  loadColumnWidths,
  maxWidth,
  saveColumnWidths,
  type ColumnWidths,
  type ResizableColumn,
  type ShellRoom,
} from "./columnWidths";

/** Same breakpoint as shell.css: below it the member list leaves the grid. */
export const NARROW_QUERY = "(max-width: 60rem)";

/** True below the narrow breakpoint, where the member column becomes an overlay. */
export function useNarrowWindow(): boolean {
  const [narrow, setNarrow] = useState(
    () => typeof window.matchMedia === "function" && window.matchMedia(NARROW_QUERY).matches,
  );
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(NARROW_QUERY);
    const update = () => {
      setNarrow(query.matches);
    };
    update();
    query.addEventListener("change", update);
    return () => {
      query.removeEventListener("change", update);
    };
  }, []);
  return narrow;
}

function rootFontPx(): number {
  return parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
}

function measureRoom(shell: HTMLElement, serverBar: HTMLElement | null): ShellRoom | null {
  // Zero width means no layout (hidden window or a test DOM): do not clamp to it.
  if (shell.clientWidth === 0) return null;
  const remPx = rootFontPx();
  const narrow = typeof window.matchMedia === "function" && window.matchMedia(NARROW_QUERY).matches;
  return {
    shell: shell.clientWidth / remPx,
    serverBar: (serverBar?.offsetWidth ?? 0) / remPx,
    membersShown: !narrow,
  };
}

const sameRoom = (a: ShellRoom | null, b: ShellRoom | null) =>
  a === b ||
  (a !== null &&
    b !== null &&
    a.shell === b.shell &&
    a.serverBar === b.serverBar &&
    a.membersShown === b.membersShown);

export interface ColumnWidthsState {
  /** Widths shown now, after fitting them into the window. */
  shown: ColumnWidths;
  /** Largest width each column may take now. */
  max: ColumnWidths;
  resize: (column: ResizableColumn, rem: number) => void;
  /** Remembers the current widths across restarts. */
  commit: () => void;
  reset: (column: ResizableColumn) => void;
}

/**
 * The user's chosen side column widths (remembered) and the widths that fit the window now. The
 * window is re-measured on resize, once per animation frame.
 */
export function useColumnWidths(
  shellRef: RefObject<HTMLElement | null>,
  serverBarRef: RefObject<HTMLElement | null>,
): ColumnWidthsState {
  const [chosen, setChosen] = useState(loadColumnWidths);
  const chosenRef = useRef(chosen);
  const [room, setRoom] = useState<ShellRoom | null>(null);

  useLayoutEffect(() => {
    const shell = shellRef.current;
    if (!shell) return;
    let frame = 0;
    const measure = () => {
      const next = measureRoom(shell, serverBarRef.current);
      setRoom((prev) => (sameRoom(prev, next) ? prev : next));
    };
    const schedule = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        measure();
      });
    };
    measure();
    window.addEventListener("resize", schedule);
    return () => {
      window.removeEventListener("resize", schedule);
      cancelAnimationFrame(frame);
    };
  }, [shellRef, serverBarRef]);

  const shown = room
    ? fitWidths(chosen, room)
    : { channels: clampWidth(chosen.channels), members: clampWidth(chosen.members) };
  const max = room
    ? { channels: maxWidth("channels", shown, room), members: maxWidth("members", shown, room) }
    : { channels: clampWidth(Infinity), members: clampWidth(Infinity) };

  const resize = useCallback((column: ResizableColumn, rem: number) => {
    const next = { ...chosenRef.current, [column]: rem };
    chosenRef.current = next;
    setChosen(next);
  }, []);

  const commit = useCallback(() => {
    saveColumnWidths(chosenRef.current);
  }, []);

  const reset = useCallback(
    (column: ResizableColumn) => {
      resize(column, COLUMN_DEFAULT_REM[column]);
      commit();
    },
    [resize, commit],
  );

  return { shown, max, resize, commit, reset };
}
